//! The turn a video asks for when shown. Phones record upright videos
//! sideways and say in the container how to turn them, as a display
//! matrix (or, in older files, a `rotate` tag); pictures decoded as stored
//! would show sideways, or squeezed into the wrong shape.

use ffmpeg::codec::packet::side_data::Type as SideData;
use ffmpeg_next as ffmpeg;

/// How far to turn a picture, clockwise, to show it upright.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Turn {
    #[default]
    None,
    Clockwise,
    Half,
    Counterclockwise,
}

impl Turn {
    /// The turn `stream` asks for.
    pub(crate) fn of(stream: &ffmpeg::format::stream::Stream<'_>) -> Self {
        let from_matrix = stream
            .side_data()
            .find(|data| data.kind() == SideData::DisplayMatrix)
            .and_then(|data| clockwise_degrees(data.data()));
        let from_tag = || {
            stream
                .metadata()
                .get("rotate")
                .and_then(|degrees| degrees.trim().parse::<f64>().ok())
        };
        from_matrix
            .or_else(from_tag)
            .map_or(Self::None, Self::nearest)
    }

    /// The quarter turn nearest `degrees` clockwise.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a rounded count of quarter turns, from a real rotation"
    )]
    fn nearest(degrees: f64) -> Self {
        match ((degrees / 90.0).round() as i64).rem_euclid(4) {
            1 => Self::Clockwise,
            2 => Self::Half,
            3 => Self::Counterclockwise,
            _ => Self::None,
        }
    }

    /// Whether turning swaps width and height.
    pub(crate) fn swaps_sides(self) -> bool {
        matches!(self, Self::Clockwise | Self::Counterclockwise)
    }

    /// `pixels`, `width` by `height` with `channels` bytes each and rows
    /// packed back to back, turned; with the turned width and height.
    pub(crate) fn apply(
        self,
        pixels: Vec<u8>,
        width: u32,
        height: u32,
        channels: usize,
    ) -> (Vec<u8>, u32, u32) {
        if self == Self::None {
            return (pixels, width, height);
        }
        let (w, h) = (width as usize, height as usize);
        let mut turned = vec![0; pixels.len()];
        for y in 0..h {
            for x in 0..w {
                let (to_x, to_y, to_width) = match self {
                    Self::Clockwise => (h - 1 - y, x, h),
                    Self::Half => (w - 1 - x, h - 1 - y, w),
                    Self::Counterclockwise => (y, w - 1 - x, h),
                    Self::None => (x, y, w),
                };
                let from = (y * w + x) * channels;
                let to = (to_y * to_width + to_x) * channels;
                turned[to..to + channels].copy_from_slice(&pixels[from..from + channels]);
            }
        }
        if self.swaps_sides() {
            (turned, height, width)
        } else {
            (turned, width, height)
        }
    }
}

/// The clockwise turn a display matrix asks for, in degrees: nine
/// 32-bit numbers, the first two holding the rotation's cosine and sine,
/// as FFmpeg's `av_display_rotation_get` reads them.
fn clockwise_degrees(matrix: &[u8]) -> Option<f64> {
    let number = |index: usize| -> Option<f64> {
        let bytes = matrix.get(index * 4..index * 4 + 4)?;
        Some(f64::from(i32::from_ne_bytes(bytes.try_into().ok()?)))
    };
    let (a, b, c, d) = (number(0)?, number(1)?, number(3)?, number(4)?);
    let (scale_a, scale_b) = (a.hypot(c), b.hypot(d));
    if scale_a == 0.0 || scale_b == 0.0 {
        return None;
    }
    Some((b / scale_b).atan2(a / scale_a).to_degrees())
}

#[cfg(test)]
mod tests {
    use super::{Turn, clockwise_degrees};

    /// A display matrix as FFmpeg writes one for `rotate` degrees.
    fn matrix(cos: i32, sin: i32) -> Vec<u8> {
        let one = 1 << 16;
        [
            cos * one,
            sin * one,
            0,
            -sin * one,
            cos * one,
            0,
            0,
            0,
            1 << 30,
        ]
        .iter()
        .flat_map(|number: &i32| number.to_ne_bytes())
        .collect()
    }

    fn turn_of(matrix: &[u8]) -> Turn {
        clockwise_degrees(matrix).map_or(Turn::None, Turn::nearest)
    }

    #[test]
    fn reads_an_upright_phone_video_as_a_clockwise_turn() {
        assert_eq!(turn_of(&matrix(0, 1)), Turn::Clockwise);
    }

    #[test]
    fn reads_the_other_turns() {
        assert_eq!(turn_of(&matrix(1, 0)), Turn::None);
        assert_eq!(turn_of(&matrix(-1, 0)), Turn::Half);
        assert_eq!(turn_of(&matrix(0, -1)), Turn::Counterclockwise);
    }

    #[test]
    fn ignores_a_broken_matrix() {
        assert_eq!(clockwise_degrees(&[0; 8]), None);
        assert_eq!(clockwise_degrees(&[0; 36]), None);
    }

    #[test]
    fn rounds_tags_to_quarter_turns() {
        assert_eq!(Turn::nearest(90.0), Turn::Clockwise);
        assert_eq!(Turn::nearest(-90.0), Turn::Counterclockwise);
        assert_eq!(Turn::nearest(450.0), Turn::Clockwise);
        assert_eq!(Turn::nearest(3.0), Turn::None);
    }

    /// A 3×2 picture, one byte per pixel:
    /// ```text
    /// 1 2 3
    /// 4 5 6
    /// ```
    fn picture() -> Vec<u8> {
        vec![1, 2, 3, 4, 5, 6]
    }

    #[test]
    fn turns_pictures_clockwise() {
        assert_eq!(
            Turn::Clockwise.apply(picture(), 3, 2, 1),
            (vec![4, 1, 5, 2, 6, 3], 2, 3)
        );
    }

    #[test]
    fn turns_pictures_counterclockwise_and_half_way() {
        assert_eq!(
            Turn::Counterclockwise.apply(picture(), 3, 2, 1),
            (vec![3, 6, 2, 5, 1, 4], 2, 3)
        );
        assert_eq!(
            Turn::Half.apply(picture(), 3, 2, 1),
            (vec![6, 5, 4, 3, 2, 1], 3, 2)
        );
    }

    #[test]
    fn moves_whole_pixels() {
        let bgra = vec![1, 1, 1, 1, 2, 2, 2, 2];
        assert_eq!(
            Turn::Clockwise.apply(bgra, 2, 1, 4),
            (vec![1, 1, 1, 1, 2, 2, 2, 2], 1, 2)
        );
    }
}
