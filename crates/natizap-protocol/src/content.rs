use whatsapp_rust::prelude::{MessageExt as _, wa};

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
        Some(Self { kind, text })
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
    fn skips_messages_without_visible_content() {
        let reaction = wa::Message {
            reaction_message: MessageField::some(wa::message::ReactionMessage::default()),
            ..Default::default()
        };
        assert_eq!(Content::of(&reaction), None);
    }
}
