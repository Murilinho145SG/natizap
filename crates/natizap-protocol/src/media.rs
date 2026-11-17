use std::sync::Arc;

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
    /// Loudness over the length of a voice note: 64 values, 0 to 100, as
    /// the sender's phone measured it.
    pub waveform: Option<Vec<u8>>,
    /// Where the encrypted file lives on WhatsApp's servers.
    pub direct_path: String,
    /// Key to decrypt the file.
    pub media_key: Vec<u8>,
    /// Hash of the decrypted file.
    pub file_sha256: Vec<u8>,
    /// Hash of the encrypted file.
    pub file_enc_sha256: Vec<u8>,
}

impl Media {
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
