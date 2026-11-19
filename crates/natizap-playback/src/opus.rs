//! Opus packets decoded one by one, as a call receives them: FFmpeg's own
//! decoder, with no container around the packets.

use ffmpeg::util::frame;
use ffmpeg::{codec, format};
use ffmpeg_next as ffmpeg;

/// Turns a stream of Opus packets into mono sound.
pub(crate) struct OpusDecoder {
    decoder: ffmpeg::decoder::Audio,
    decoded: frame::Audio,
}

impl OpusDecoder {
    pub(crate) fn new() -> Result<Self, ffmpeg::Error> {
        ffmpeg::init()?;
        let codec = ffmpeg::decoder::find(codec::Id::OPUS).ok_or(ffmpeg::Error::DecoderNotFound)?;
        let decoder = codec::context::Context::new_with_codec(codec)
            .decoder()
            .audio()?;
        Ok(Self {
            decoder,
            decoded: frame::Audio::empty(),
        })
    }

    /// Decodes `packet`, appending its sound, mono, to `sound`; returns
    /// the sound's rate, once there is some.
    pub(crate) fn decode(
        &mut self,
        packet: &[u8],
        sound: &mut Vec<f32>,
    ) -> Result<Option<u32>, ffmpeg::Error> {
        self.decoder.send_packet(&ffmpeg::Packet::copy(packet))?;
        let mut rate = None;
        while self.decoder.receive_frame(&mut self.decoded).is_ok() {
            rate = Some(self.decoded.rate());
            mono(&self.decoded, sound);
        }
        Ok(rate)
    }
}

/// Appends `decoded`'s samples to `sound`, its channels averaged. Formats
/// other than 32-bit float, which FFmpeg's Opus decoder does not give, are
/// skipped.
#[expect(
    clippy::cast_precision_loss,
    reason = "a sound card has a handful of channels"
)]
fn mono(decoded: &frame::Audio, sound: &mut Vec<f32>) {
    let channels = usize::from(decoded.channels()).max(1);
    let samples = decoded.samples();
    let floats = |index: usize, count: usize| -> Vec<f32> {
        decoded.data(index)[..count * 4]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&bytes| f32::from_ne_bytes(bytes))
            .collect()
    };
    match decoded.format() {
        format::Sample::F32(format::sample::Type::Planar) => {
            let planes: Vec<Vec<f32>> = (0..channels).map(|plane| floats(plane, samples)).collect();
            sound.extend(
                (0..samples)
                    .map(|at| planes.iter().map(|plane| plane[at]).sum::<f32>() / channels as f32),
            );
        }
        format::Sample::F32(format::sample::Type::Packed) => {
            sound.extend(
                floats(0, samples * channels)
                    .chunks(channels)
                    .map(|frame| frame.iter().sum::<f32>() / channels as f32),
            );
        }
        other => tracing::warn!(?other, "Opus decoded to an unexpected sample format"),
    }
}

#[cfg(test)]
mod tests {
    use ffmpeg::{ChannelLayout, Dictionary};

    use super::*;

    /// A tone as Opus packets of 20 ms, the way FFmpeg's encoder cuts it.
    fn packets() -> Result<Vec<Vec<u8>>, ffmpeg::Error> {
        ffmpeg::init()?;
        let codec = ffmpeg::encoder::find(codec::Id::OPUS).ok_or(ffmpeg::Error::EncoderNotFound)?;
        let mut encoder = codec::context::Context::new_with_codec(codec)
            .encoder()
            .audio()?;
        encoder.set_rate(48_000);
        encoder.set_channel_layout(ChannelLayout::MONO);
        encoder.set_format(format::Sample::F32(format::sample::Type::Planar));
        encoder.set_bit_rate(32_000);
        encoder.set_time_base((1, 48_000));
        let mut options = Dictionary::new();
        options.set("strict", "experimental");
        let mut encoder = encoder.open_as_with(codec, options)?;
        let size = usize::try_from(encoder.frame_size()).unwrap_or(960);
        let mut packets = Vec::new();
        let mut packet = ffmpeg::Packet::empty();
        for index in 0..50_usize {
            let mut audio = frame::Audio::new(
                format::Sample::F32(format::sample::Type::Planar),
                size,
                ChannelLayout::MONO,
            );
            audio.set_rate(48_000);
            audio.set_pts(i64::try_from(index * size).ok());
            #[expect(clippy::cast_precision_loss, reason = "test indices are small")]
            for (at, sample) in audio.plane_mut::<f32>(0).iter_mut().enumerate() {
                let time = (index * size + at) as f32 / 48_000.0;
                *sample = 0.5 * (std::f32::consts::TAU * 440.0 * time).sin();
            }
            encoder.send_frame(&audio)?;
            while encoder.receive_packet(&mut packet).is_ok() {
                packets.push(packet.data().unwrap_or_default().to_vec());
            }
        }
        Ok(packets)
    }

    #[test]
    fn decodes_packets_without_a_container() -> Result<(), Box<dyn std::error::Error>> {
        let mut decoder = OpusDecoder::new()?;
        let mut sound = Vec::new();
        let mut rate = None;
        for packet in packets()? {
            rate = decoder.decode(&packet, &mut sound)?.or(rate);
        }
        assert_eq!(rate, Some(48_000));
        // A second went in; the encoder, never told it ended, still holds
        // the last part of it.
        assert!(sound.len() > 24_000, "{}", sound.len());
        let steady = &sound[sound.len() / 2..];
        #[expect(clippy::cast_precision_loss, reason = "test signals are short")]
        let rms = (steady.iter().map(|s| s * s).sum::<f32>() / steady.len() as f32).sqrt();
        assert!((rms - 0.35).abs() < 0.08, "{rms}");
        Ok(())
    }
}
