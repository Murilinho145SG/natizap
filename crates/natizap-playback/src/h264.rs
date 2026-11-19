//! H.264 for a call's camera: `OpenH264`'s encoder, set as WhatsApp's own
//! clients send: Constrained Baseline at about fifteen pictures a second,
//! a keyframe every two seconds and whenever the other end asks.

use ffmpeg::util::frame;
use ffmpeg_next as ffmpeg;
use openh264::OpenH264API;
use openh264::encoder::{
    BitRate, Encoder, EncoderConfig, FrameRate, IntraFramePeriod, Profile, RateControlMode,
    UsageType,
};
use openh264::formats::YUVSlices;

/// Pictures a second a call's camera sends: the cadence the call's RTP
/// clock assumes unless told otherwise.
pub(crate) const FRAME_RATE: u32 = 15;
/// What a call's camera spends, in bits a second: a clear 480p.
const BITRATE: u32 = 700_000;
/// Pictures between two keyframes, so a lost one heals within seconds.
const KEYFRAME_EVERY: u32 = 2 * FRAME_RATE;

/// Turns YUV 4:2:0 pictures into Annex-B access units.
pub(crate) struct H264Encoder {
    encoder: Encoder,
}

impl H264Encoder {
    pub(crate) fn new() -> Result<Self, openh264::Error> {
        #[expect(clippy::cast_precision_loss, reason = "a small frame rate")]
        let rate = FrameRate::from_hz(FRAME_RATE as f32);
        let config = EncoderConfig::new()
            .bitrate(BitRate::from_bps(BITRATE))
            .max_frame_rate(rate)
            .usage_type(UsageType::CameraVideoRealTime)
            .rate_control_mode(RateControlMode::Bitrate)
            .profile(Profile::Baseline)
            .intra_frame_period(IntraFramePeriod::from_num_frames(KEYFRAME_EVERY))
            // Every picture is sent: the call's clock counts them.
            .skip_frames(false);
        Ok(Self {
            encoder: Encoder::with_api_config(OpenH264API::from_source(), config)?,
        })
    }

    /// `picture`, a YUV 4:2:0 frame with even sides, as one access unit; a
    /// keyframe when `keyframe` asks for one.
    pub(crate) fn encode(
        &mut self,
        picture: &frame::Video,
        keyframe: bool,
    ) -> Result<Vec<u8>, openh264::Error> {
        if keyframe {
            self.encoder.force_intra_frame();
        }
        let slices = YUVSlices::new(
            (picture.data(0), picture.data(1), picture.data(2)),
            (picture.width() as usize, picture.height() as usize),
            (picture.stride(0), picture.stride(1), picture.stride(2)),
        );
        Ok(self.encoder.encode(&slices)?.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use ffmpeg::format::Pixel;
    use ffmpeg::util::frame;
    use ffmpeg_next as ffmpeg;

    use super::H264Encoder;
    use crate::remote_video::Decoder;
    use crate::turn::Turn;

    /// A picture whose shade moves with `index`.
    fn picture(index: u8) -> frame::Video {
        let mut picture = frame::Video::new(Pixel::YUV420P, 320, 240);
        for plane in 0..3 {
            let shade = if plane == 0 {
                index.wrapping_mul(8)
            } else {
                128
            };
            picture.data_mut(plane).fill(shade);
        }
        picture
    }

    #[test]
    fn what_the_camera_encodes_the_other_end_decodes() -> Result<(), Box<dyn std::error::Error>> {
        let mut encoder = H264Encoder::new()?;
        let mut decoder = Decoder::new()?;
        let mut shown = None;
        for index in 0..10 {
            let unit = encoder.encode(&picture(index), index == 0)?;
            assert!(unit.starts_with(&[0, 0, 0, 1]), "not Annex-B");
            shown = decoder.decode(&unit, Turn::None)?.or(shown);
        }
        let shown = shown.ok_or("nothing decoded")?;
        assert_eq!((shown.width, shown.height), (320, 240));
        assert_eq!(shown.bgra.len(), 320 * 240 * 4);
        Ok(())
    }

    #[test]
    fn a_phone_held_sideways_is_shown_upright() -> Result<(), Box<dyn std::error::Error>> {
        let mut encoder = H264Encoder::new()?;
        let mut decoder = Decoder::new()?;
        let mut shown = None;
        for index in 0..5 {
            let unit = encoder.encode(&picture(index), index == 0)?;
            shown = decoder.decode(&unit, Turn::Clockwise)?.or(shown);
        }
        let shown = shown.ok_or("nothing decoded")?;
        assert_eq!((shown.width, shown.height), (240, 320));
        Ok(())
    }
}
