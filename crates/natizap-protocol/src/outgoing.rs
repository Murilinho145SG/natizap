//! Sending photos, videos, voice notes, audio, documents and stickers.

use std::sync::Arc;

use whatsapp_rust::Jid;
use whatsapp_rust::prelude::{Client, MessageExt as _, MessageField, wa};
use whatsapp_rust::upload::{UploadOptions, UploadResponse};
use whatsapp_rust::wacore::download::MediaType;

use crate::actions::{ActionError, Reply, reply_context};
use crate::{ContentKind, Media};

/// A file to send, with what the other side shows before downloading it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outgoing {
    /// What it is; text, locations, contacts and polls are not files.
    pub kind: ContentKind,
    /// The file itself.
    pub bytes: Vec<u8>,
    /// MIME type, e.g. `image/jpeg`.
    pub mime: String,
    /// The name documents are shown and saved under.
    pub file_name: Option<String>,
    /// Text under a photo, video or document.
    pub caption: Option<String>,
    /// A small JPEG preview, for photos, videos and documents.
    pub thumbnail: Option<Vec<u8>>,
    /// Width in pixels, for photos, videos and stickers.
    pub width: Option<u32>,
    /// Height in pixels, for photos, videos and stickers.
    pub height: Option<u32>,
    /// Length in seconds, for videos and sound.
    pub seconds: Option<u32>,
    /// Loudness bars of a voice note, 0 to 100 each.
    pub waveform: Option<Vec<u8>>,
}

/// A file WhatsApp accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentMedia {
    /// WhatsApp's id for the message.
    pub id: String,
    /// Where the file now lives, as a received message would describe it.
    pub media: Media,
}

pub(crate) async fn send(
    client: Arc<Client>,
    to: Jid,
    mut outgoing: Outgoing,
    reply: Option<Reply>,
) -> Result<SentMedia, ActionError> {
    let rejected = |error: &dyn std::fmt::Display| ActionError::Rejected(error.to_string());
    let media_type = match outgoing.kind {
        ContentKind::Photo => MediaType::Image,
        ContentKind::Video => MediaType::Video,
        ContentKind::VoiceMessage | ContentKind::Audio => MediaType::Audio,
        ContentKind::Sticker => MediaType::Sticker,
        _ => MediaType::Document,
    };
    // Moved, not copied: a video can be large.
    let bytes = std::mem::take(&mut outgoing.bytes);
    let upload = client
        .upload(bytes, media_type, UploadOptions::default())
        .await
        .map_err(|error| rejected(&error))?;
    let media = Media {
        mime: Some(outgoing.mime.clone()),
        file_name: outgoing.file_name.clone(),
        size: Some(upload.file_length),
        width: outgoing.width,
        height: outgoing.height,
        seconds: outgoing.seconds,
        thumbnail: outgoing.thumbnail.clone(),
        hd_thumbnail: None,
        waveform: outgoing.waveform.clone(),
        direct_path: upload.direct_path.clone(),
        media_key: upload.media_key.to_vec(),
        file_sha256: upload.file_sha256.to_vec(),
        file_enc_sha256: upload.file_enc_sha256.to_vec(),
    };
    let mut message = message(outgoing, upload);
    if let Some(reply) = reply {
        message.set_context_info(reply_context(&client, reply)?);
    }
    let sent = client
        .send_message(to, message)
        .await
        .map_err(|error| rejected(&error))?;
    Ok(SentMedia {
        id: sent.message_id,
        media,
    })
}

/// The message carrying `o`, uploaded as `u`.
fn message(o: Outgoing, u: UploadResponse) -> wa::Message {
    let mut message = wa::Message::default();
    match o.kind {
        ContentKind::Photo => {
            message.image_message = MessageField::some(wa::message::ImageMessage {
                url: Some(u.url),
                mimetype: Some(o.mime),
                caption: o.caption,
                file_sha256: Some(u.file_sha256.to_vec()),
                file_length: Some(u.file_length),
                height: o.height,
                width: o.width,
                media_key: Some(u.media_key.to_vec()),
                file_enc_sha256: Some(u.file_enc_sha256.to_vec()),
                direct_path: Some(u.direct_path),
                media_key_timestamp: Some(u.media_key_timestamp),
                jpeg_thumbnail: o.thumbnail,
                ..Default::default()
            });
        }
        ContentKind::Video => {
            message.video_message = MessageField::some(wa::message::VideoMessage {
                url: Some(u.url),
                mimetype: Some(o.mime),
                file_sha256: Some(u.file_sha256.to_vec()),
                file_length: Some(u.file_length),
                seconds: o.seconds,
                media_key: Some(u.media_key.to_vec()),
                caption: o.caption,
                height: o.height,
                width: o.width,
                file_enc_sha256: Some(u.file_enc_sha256.to_vec()),
                direct_path: Some(u.direct_path),
                media_key_timestamp: Some(u.media_key_timestamp),
                jpeg_thumbnail: o.thumbnail,
                streaming_sidecar: u.streaming_sidecar,
                ..Default::default()
            });
        }
        ContentKind::VoiceMessage | ContentKind::Audio => {
            message.audio_message = MessageField::some(wa::message::AudioMessage {
                url: Some(u.url),
                mimetype: Some(o.mime),
                file_sha256: Some(u.file_sha256.to_vec()),
                file_length: Some(u.file_length),
                seconds: o.seconds,
                ptt: Some(o.kind == ContentKind::VoiceMessage),
                media_key: Some(u.media_key.to_vec()),
                file_enc_sha256: Some(u.file_enc_sha256.to_vec()),
                direct_path: Some(u.direct_path),
                media_key_timestamp: Some(u.media_key_timestamp),
                streaming_sidecar: u.streaming_sidecar,
                waveform: o.waveform,
                ..Default::default()
            });
        }
        ContentKind::Sticker => {
            message.sticker_message = MessageField::some(wa::message::StickerMessage {
                url: Some(u.url),
                file_sha256: Some(u.file_sha256.to_vec()),
                file_enc_sha256: Some(u.file_enc_sha256.to_vec()),
                media_key: Some(u.media_key.to_vec()),
                mimetype: Some(o.mime),
                height: o.height,
                width: o.width,
                direct_path: Some(u.direct_path),
                file_length: Some(u.file_length),
                media_key_timestamp: Some(u.media_key_timestamp),
                ..Default::default()
            });
        }
        // Anything else goes as a document, which takes any file.
        _ => {
            message.document_message = MessageField::some(wa::message::DocumentMessage {
                url: Some(u.url),
                mimetype: Some(o.mime),
                title: o.file_name.clone(),
                file_sha256: Some(u.file_sha256.to_vec()),
                file_length: Some(u.file_length),
                media_key: Some(u.media_key.to_vec()),
                file_name: o.file_name,
                file_enc_sha256: Some(u.file_enc_sha256.to_vec()),
                direct_path: Some(u.direct_path),
                media_key_timestamp: Some(u.media_key_timestamp),
                jpeg_thumbnail: o.thumbnail,
                caption: o.caption,
                ..Default::default()
            });
        }
    }
    message
}
