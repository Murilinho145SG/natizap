use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use whatsapp_rust::download::DownloadParams;
use whatsapp_rust::prelude::{Client, wa};
use whatsapp_rust::wacore::download::MediaType;

use crate::{ActionError, ContentKind};

/// A photo, video, voice note, audio file, document or sticker attached to a
/// message: what it is, a preview, and what is needed to download it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Media {
    /// MIME type, e.g. `image/jpeg`.
    pub mime: Option<String>,
    /// Original file name, for documents.
    pub file_name: Option<String>,
    /// Size in bytes.
    pub size: Option<u64>,
    /// Width in pixels, for images, videos and stickers.
    pub width: Option<u32>,
    /// Height in pixels, for images, videos and stickers.
    pub height: Option<u32>,
    /// Length in seconds, for videos and audio.
    pub seconds: Option<u32>,
    /// A tiny inline preview (JPEG, or PNG for stickers) that needs no
    /// download.
    pub thumbnail: Option<Vec<u8>>,
    /// A sharper preview of a video, stored apart from the video.
    pub hd_thumbnail: Option<HdThumbnail>,
    /// Loudness over the length of a voice note: 64 values, 0 to 100, as
    /// the sender's phone measured it.
    pub waveform: Option<Vec<u8>>,
    /// How many pages a document has.
    pub pages: Option<u32>,
    /// Where the encrypted file lives on WhatsApp's servers.
    pub direct_path: String,
    /// Key to decrypt the file.
    pub media_key: Vec<u8>,
    /// Hash of the decrypted file.
    pub file_sha256: Vec<u8>,
    /// Hash of the encrypted file.
    pub file_enc_sha256: Vec<u8>,
}

/// Where a video's sharper preview lives on WhatsApp's servers. It is
/// encrypted with the video's key.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HdThumbnail {
    /// Where the encrypted preview lives.
    pub direct_path: String,
    /// Hash of the decrypted preview.
    pub sha256: Vec<u8>,
    /// Hash of the encrypted preview.
    pub enc_sha256: Vec<u8>,
}

impl HdThumbnail {
    /// `video`'s sharper preview, when it says where one is. Phones leave
    /// it out at times, e.g. for forwarded or old videos.
    fn of(video: &wa::message::VideoMessage) -> Option<Self> {
        Some(Self {
            direct_path: video.thumbnail_direct_path.clone()?,
            sha256: video.thumbnail_sha256.clone()?,
            enc_sha256: video.thumbnail_enc_sha256.clone()?,
        })
    }
}

impl Media {
    /// Whether the link to the file on WhatsApp's servers has expired. The
    /// file cannot be downloaded through it any more, and every try makes
    /// the client ask for a new media connection, which WhatsApp
    /// rate-limits for the whole account.
    #[must_use]
    pub fn link_expired(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_secs());
        expires_at(&self.direct_path).is_some_and(|at| at <= now)
    }

    /// The sharper preview as an attachment of its own, to hand to
    /// [`crate::Protocol::download`] with the video's kind.
    #[must_use]
    pub fn hd_thumbnail_download(&self) -> Option<Self> {
        let thumbnail = self.hd_thumbnail.as_ref()?;
        Some(Self {
            mime: Some("image/jpeg".to_owned()),
            direct_path: thumbnail.direct_path.clone(),
            media_key: self.media_key.clone(),
            file_sha256: thumbnail.sha256.clone(),
            file_enc_sha256: thumbnail.enc_sha256.clone(),
            ..Self::default()
        })
    }

    /// The attachment of `message`, if it has a downloadable one.
    pub(crate) fn of(message: &wa::Message) -> Option<Self> {
        macro_rules! locator {
            ($m:expr) => {
                Self {
                    mime: $m.mimetype.clone(),
                    size: $m.file_length,
                    direct_path: $m.direct_path.clone()?,
                    media_key: $m.media_key.clone()?,
                    file_sha256: $m.file_sha256.clone()?,
                    file_enc_sha256: $m.file_enc_sha256.clone()?,
                    ..Self::default()
                }
            };
        }
        if let Some(m) = message.image_message.as_option() {
            Some(Self {
                width: m.width,
                height: m.height,
                thumbnail: m.jpeg_thumbnail.clone(),
                ..locator!(m)
            })
        } else if let Some(m) = message.video_message.as_option() {
            Some(Self {
                width: m.width,
                height: m.height,
                seconds: m.seconds,
                thumbnail: m.jpeg_thumbnail.clone(),
                hd_thumbnail: HdThumbnail::of(m),
                ..locator!(m)
            })
        } else if let Some(m) = message.audio_message.as_option() {
            Some(Self {
                seconds: m.seconds,
                waveform: m.waveform.clone(),
                ..locator!(m)
            })
        } else if let Some(m) = message.document_message.as_option() {
            Some(Self {
                file_name: m.file_name.clone(),
                thumbnail: m.jpeg_thumbnail.clone(),
                pages: m.page_count,
                ..locator!(m)
            })
        } else if let Some(m) = message.sticker_message.as_option() {
            Some(Self {
                width: m.width,
                height: m.height,
                thumbnail: m.png_thumbnail.clone(),
                ..locator!(m)
            })
        } else {
            None
        }
    }
}

pub(crate) async fn download(
    client: Arc<Client>,
    kind: ContentKind,
    media: Media,
) -> Result<Vec<u8>, ActionError> {
    let media_type = match kind {
        ContentKind::Photo => MediaType::Image,
        ContentKind::Video => MediaType::Video,
        ContentKind::VoiceMessage | ContentKind::Audio => MediaType::Audio,
        ContentKind::Sticker => MediaType::Sticker,
        _ => MediaType::Document,
    };
    let params = DownloadParams::encrypted(
        media.direct_path,
        &media.media_key,
        &media.file_sha256,
        &media.file_enc_sha256,
        media.size.unwrap_or_default(),
        media_type,
    );
    client
        .download(&params)
        .await
        .map_err(|error| ActionError::Rejected(error.to_string()))
}

/// When the link `direct_path` stops working, in seconds since the epoch:
/// its `oe` parameter, in hexadecimal. A link without one does not say.
fn expires_at(direct_path: &str) -> Option<u64> {
    let (_, query) = direct_path.split_once('?')?;
    let hex = query.split('&').find_map(|pair| pair.strip_prefix("oe="))?;
    u64::from_str_radix(hex, 16).ok()
}

#[cfg(test)]
mod tests {
    use super::{Media, expires_at};

    #[test]
    fn reads_when_a_link_expires() {
        assert_eq!(
            expires_at("/v/t62.15575-24/1_2_n.enc?ccb=11-4&oh=01_Q5&oe=6A1B2C3D&_nc_sid=5e03e0"),
            Some(0x6A1B_2C3D)
        );
        assert_eq!(expires_at("/v/t62.15575-24/1_2_n.enc?ccb=11-4"), None);
        assert_eq!(expires_at("/mms/image/abc"), None);
    }

    #[test]
    fn tells_expired_links_from_live_ones() {
        let link = |oe: &str| Media {
            direct_path: format!("/v/t62.15575-24/1.enc?ccb=11-4&oe={oe}"),
            ..Media::default()
        };
        assert!(link("5F000000").link_expired());
        assert!(!link("FFFFFFFF").link_expired());
        assert!(!Media::default().link_expired());
    }
}
