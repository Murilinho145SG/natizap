//! Sending photos, videos, voice notes, audio, documents and stickers.

use std::sync::Arc;

use whatsapp_rust::Jid;
use whatsapp_rust::prelude::{Client, MessageBuilderExt as _, MessageExt as _, MessageField, wa};
use whatsapp_rust::upload::UploadOptions;
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

/// A message to send again to another chat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Forward {
    /// What it carries.
    pub kind: ContentKind,
    /// Its text or caption.
    pub text: Option<String>,
    /// Its file, already on WhatsApp's servers; `None` for text.
    pub media: Option<Media>,
}

/// Sends `forward` to `to`, marked as forwarded. A file is not uploaded
/// again: the message points at the copy already on WhatsApp's servers, as
/// the phone does.
pub(crate) async fn forward(
    client: Arc<Client>,
    to: Jid,
    forward: Forward,
) -> Result<String, ActionError> {
    let context = wa::ContextInfo {
        is_forwarded: Some(true),
        forwarding_score: Some(1),
        ..Default::default()
    };
    let mut message = match forward.media {
        Some(media) => media_message(forward.kind, &media, forward.text, None),
        None => wa::Message::text(forward.text.unwrap_or_default()),
    };
    message.set_context_info(context);
    let sent = client
        .send_message(to, message)
        .await
        .map_err(|error| ActionError::Rejected(error.to_string()))?;
    Ok(sent.message_id)
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
    let fresh = Fresh {
        url: upload.url,
        timestamp: upload.media_key_timestamp,
        sidecar: upload.streaming_sidecar,
    };
    let mut message = media_message(outgoing.kind, &media, outgoing.caption, Some(fresh));
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

/// What only a fresh upload knows; a forwarded file goes without.
pub(crate) struct Fresh {
    url: String,
    timestamp: i64,
    sidecar: Option<Vec<u8>>,
}

/// The message carrying the file `media` describes, as a `kind` message
/// with `caption`.
pub(crate) fn media_message(
    kind: ContentKind,
    media: &Media,
    caption: Option<String>,
    fresh: Option<Fresh>,
) -> wa::Message {
    let m = media.clone();
    let (url, timestamp, sidecar) = match fresh {
        Some(fresh) => (Some(fresh.url), Some(fresh.timestamp), fresh.sidecar),
        None => (None, None, None),
    };
    let mut message = wa::Message::default();
    match kind {
        ContentKind::Photo => {
            message.image_message = MessageField::some(wa::message::ImageMessage {
                url,
                mimetype: m.mime,
                caption,
                file_sha256: Some(m.file_sha256),
                file_length: m.size,
                height: m.height,
                width: m.width,
                media_key: Some(m.media_key),
                file_enc_sha256: Some(m.file_enc_sha256),
                direct_path: Some(m.direct_path),
                media_key_timestamp: timestamp,
                jpeg_thumbnail: m.thumbnail,
                ..Default::default()
            });
        }
        ContentKind::Video => {
            message.video_message = MessageField::some(wa::message::VideoMessage {
                url,
                mimetype: m.mime,
                file_sha256: Some(m.file_sha256),
                file_length: m.size,
                seconds: m.seconds,
                media_key: Some(m.media_key),
                caption,
                height: m.height,
                width: m.width,
                file_enc_sha256: Some(m.file_enc_sha256),
                direct_path: Some(m.direct_path),
                media_key_timestamp: timestamp,
                jpeg_thumbnail: m.thumbnail,
                streaming_sidecar: sidecar,
                ..Default::default()
            });
        }
        ContentKind::VoiceMessage | ContentKind::Audio => {
            message.audio_message = MessageField::some(wa::message::AudioMessage {
                url,
                mimetype: m.mime,
                file_sha256: Some(m.file_sha256),
                file_length: m.size,
                seconds: m.seconds,
                ptt: Some(kind == ContentKind::VoiceMessage),
                media_key: Some(m.media_key),
                file_enc_sha256: Some(m.file_enc_sha256),
                direct_path: Some(m.direct_path),
                media_key_timestamp: timestamp,
                streaming_sidecar: sidecar,
                waveform: m.waveform,
                ..Default::default()
            });
        }
        ContentKind::Sticker => {
            message.sticker_message = MessageField::some(wa::message::StickerMessage {
                url,
                file_sha256: Some(m.file_sha256),
                file_enc_sha256: Some(m.file_enc_sha256),
                media_key: Some(m.media_key),
                mimetype: m.mime,
                height: m.height,
                width: m.width,
                direct_path: Some(m.direct_path),
                file_length: m.size,
                media_key_timestamp: timestamp,
                ..Default::default()
            });
        }
        // Anything else goes as a document, which takes any file.
        _ => {
            message.document_message = MessageField::some(wa::message::DocumentMessage {
                url,
                mimetype: m.mime,
                title: m.file_name.clone(),
                file_sha256: Some(m.file_sha256),
                file_length: m.size,
                media_key: Some(m.media_key),
                file_name: m.file_name,
                file_enc_sha256: Some(m.file_enc_sha256),
                direct_path: Some(m.direct_path),
                media_key_timestamp: timestamp,
                jpeg_thumbnail: m.thumbnail,
                caption,
                ..Default::default()
            });
        }
    }
    message
}
