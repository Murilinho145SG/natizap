use whatsapp_rust::prelude::{MessageExt as _, wa};

use crate::ChatId;
use crate::media::Media;

/// What a message carries, as far as the interface needs to know.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentKind {
    /// Plain or formatted text.
    Text,
    /// An image.
    Photo,
    /// A video or GIF.
    Video,
    /// A recorded voice note.
    VoiceMessage,
    /// An audio file.
    Audio,
    /// A file.
    Document,
    /// A sticker.
    Sticker,
    /// A shared or live location.
    Location,
    /// A shared contact card.
    Contact,
    /// A poll.
    Poll,
}

impl ContentKind {
    /// Every kind, e.g. to read a stored [`ContentKind::label`] back.
    pub const ALL: [Self; 10] = [
        Self::Text,
        Self::Photo,
        Self::Video,
        Self::VoiceMessage,
        Self::Audio,
        Self::Document,
        Self::Sticker,
        Self::Location,
        Self::Contact,
        Self::Poll,
    ];

    /// The kind whose [`ContentKind::label`] is `label`.
    #[must_use]
    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.label() == label)
    }

    /// Short name, shown where the content itself cannot be.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Text => "Message",
            Self::Photo => "Photo",
            Self::Video => "Video",
            Self::VoiceMessage => "Voice message",
            Self::Audio => "Audio",
            Self::Document => "Document",
            Self::Sticker => "Sticker",
            Self::Location => "Location",
            Self::Contact => "Contact",
            Self::Poll => "Poll",
        }
    }
}

/// The visible content of a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Content {
    /// What kind of message it is.
    pub kind: ContentKind,
    /// The text, or a media caption.
    pub text: Option<String>,
    /// The message this one replies to.
    pub quote: Option<Quote>,
    /// The attached photo, video, audio, document or sticker.
    pub media: Option<Media>,
}

/// A message being replied to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Quote {
    /// WhatsApp's id of the quoted message.
    pub id: String,
    /// Who wrote it, when WhatsApp says.
    pub sender: Option<ChatId>,
    /// One line describing it, as the replier's phone saw it.
    pub summary: Option<String>,
}

impl Quote {
    fn of(message: &wa::Message) -> Option<Self> {
        let context = context_info(message)?;
        Some(Self {
            id: context.stanza_id.clone()?,
            sender: context.participant.as_deref().map(ChatId::new),
            summary: context
                .quoted_message
                .as_option()
                .and_then(Content::of)
                .map(|c| c.summary()),
        })
    }
}

/// The reply metadata, wherever the message kind keeps it.
fn context_info(message: &wa::Message) -> Option<&wa::ContextInfo> {
    let infos = [
        message
            .extended_text_message
            .as_option()
            .map(|m| &m.context_info),
        message.image_message.as_option().map(|m| &m.context_info),
        message.video_message.as_option().map(|m| &m.context_info),
        message.audio_message.as_option().map(|m| &m.context_info),
        message
            .document_message
            .as_option()
            .map(|m| &m.context_info),
        message.sticker_message.as_option().map(|m| &m.context_info),
        message
            .location_message
            .as_option()
            .map(|m| &m.context_info),
        message.contact_message.as_option().map(|m| &m.context_info),
    ];
    infos
        .into_iter()
        .flatten()
        .find_map(|info| info.as_option())
}

impl Content {
    /// Reads the content of `message`, or `None` for messages that are not
    /// shown on their own (reactions, edits, protocol housekeeping).
    #[must_use]
    pub fn of(message: &wa::Message) -> Option<Self> {
        let message = message.get_base_message();
        let text = message
            .text_content()
            .or_else(|| message.get_caption())
            .map(str::to_owned);
        let kind = if message.image_message.is_set() {
            ContentKind::Photo
        } else if message.video_message.is_set() {
            ContentKind::Video
        } else if let Some(audio) = message.audio_message.as_option() {
            if audio.ptt.unwrap_or(false) {
                ContentKind::VoiceMessage
            } else {
                ContentKind::Audio
            }
        } else if message.document_message.is_set()
            || message.document_with_caption_message.is_set()
        {
            ContentKind::Document
        } else if message.sticker_message.is_set() {
            ContentKind::Sticker
        } else if message.location_message.is_set() || message.live_location_message.is_set() {
            ContentKind::Location
        } else if message.contact_message.is_set() {
            ContentKind::Contact
        } else if message.poll_creation_message.is_set()
            || message.poll_creation_message_v2.is_set()
            || message.poll_creation_message_v3.is_set()
        {
            ContentKind::Poll
        } else if text.is_some() {
            ContentKind::Text
        } else {
            return None;
        };
        Some(Self {
            kind,
            text,
            quote: Quote::of(message),
            media: Media::of(message),
        })
    }

    /// One line describing the content: its text, or what kind it is.
    #[must_use]
    pub fn summary(&self) -> String {
        match (&self.text, self.kind) {
            (Some(text), ContentKind::Text) => text.clone(),
            (Some(caption), kind) => format!("{}: {caption}", kind.label()),
            (None, kind) => kind.label().to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use whatsapp_rust::prelude::{MessageBuilderExt as _, MessageField};

    use super::*;

    #[test]
    fn reads_text_and_describes_media() {
        let text = Content::of(&wa::Message::text("hello"));
        assert_eq!(
            text.as_ref().map(Content::summary).as_deref(),
            Some("hello")
        );

        let photo = wa::Message {
            image_message: MessageField::some(wa::message::ImageMessage {
                caption: Some("beach".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let photo = Content::of(&photo);
        assert_eq!(photo.as_ref().map(|c| c.kind), Some(ContentKind::Photo));
        assert_eq!(photo.map(|c| c.summary()).as_deref(), Some("Photo: beach"));
    }

    #[test]
    fn every_kind_has_a_distinct_label() {
        for kind in ContentKind::ALL {
            assert_eq!(ContentKind::from_label(kind.label()), Some(kind));
        }
    }

    #[test]
    fn reads_the_quoted_message_of_a_reply() {
        let reply = wa::Message {
            extended_text_message: MessageField::some(wa::message::ExtendedTextMessage {
                text: Some("agreed".into()),
                context_info: MessageField::some(wa::ContextInfo {
                    stanza_id: Some("3EB0".into()),
                    participant: Some("5511:2@s.whatsapp.net".into()),
                    quoted_message: MessageField::some(wa::Message::text("lunch?")),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        let quote = Content::of(&reply).and_then(|content| content.quote);
        assert_eq!(
            quote,
            Some(Quote {
                id: "3EB0".into(),
                sender: Some(ChatId::new("5511@s.whatsapp.net")),
                summary: Some("lunch?".into()),
            })
        );
    }

    #[test]
    fn carries_what_is_needed_to_show_and_download_a_photo() {
        let image = |direct_path: Option<&str>| wa::Message {
            image_message: MessageField::some(wa::message::ImageMessage {
                direct_path: direct_path.map(str::to_owned),
                media_key: Some(vec![1]),
                file_sha256: Some(vec![2]),
                file_enc_sha256: Some(vec![3]),
                width: Some(800),
                height: Some(600),
                jpeg_thumbnail: Some(vec![0xff, 0xd8]),
                ..Default::default()
            }),
            ..Default::default()
        };

        let media = Content::of(&image(Some("/v/t62/abc"))).and_then(|content| content.media);
        let media = media.unwrap_or_else(|| panic!("a complete photo carries media"));
        assert_eq!((media.width, media.height), (Some(800), Some(600)));
        assert_eq!(media.thumbnail.as_deref(), Some(&[0xff, 0xd8][..]));

        // Without a location the file cannot be fetched, but the photo is
        // still shown by kind.
        let unreachable = Content::of(&image(None));
        assert_eq!(
            unreachable.as_ref().map(|content| content.kind),
            Some(ContentKind::Photo)
        );
        assert_eq!(unreachable.and_then(|content| content.media), None);
    }

    #[test]
    fn skips_messages_without_visible_content() {
        let reaction = wa::Message {
            reaction_message: MessageField::some(wa::message::ReactionMessage::default()),
            ..Default::default()
        };
        assert_eq!(Content::of(&reaction), None);
    }
}
