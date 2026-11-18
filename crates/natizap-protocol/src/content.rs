use whatsapp_rust::prelude::{MessageExt as _, wa};

use crate::LinkPreview;

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
    /// A voice call nobody answered.
    MissedVoiceCall,
    /// A video call nobody answered.
    MissedVideoCall,
}

impl ContentKind {
    /// Every kind, e.g. to read a stored [`ContentKind::label`] back.
    pub const ALL: [Self; 12] = [
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
        Self::MissedVoiceCall,
        Self::MissedVideoCall,
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
            Self::MissedVoiceCall => "Missed voice call",
            Self::MissedVideoCall => "Missed video call",
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
    /// Shared contacts, or a poll.
    pub extra: Option<Extra>,
    /// The message was forwarded, and how often.
    pub forwarding: Option<Forwarding>,
    /// The preview of a link in the text.
    pub link: Option<LinkPreview>,
    /// The secret votes on a poll are sealed with.
    pub secret: Option<Vec<u8>>,
}

/// How far a forwarded message has travelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Forwarding {
    /// Forwarded.
    Once,
    /// Forwarded through a chain of at least five chats: "Forwarded many
    /// times".
    Often,
}

impl Forwarding {
    fn of(message: &wa::Message) -> Option<Self> {
        let context = context_info(message)?;
        if context.is_forwarded != Some(true) {
            return None;
        }
        // WhatsApp counts each forward and warns from the fifth on.
        Some(if context.forwarding_score.unwrap_or_default() >= 5 {
            Self::Often
        } else {
            Self::Once
        })
    }
}

/// What a contact or poll message carries besides text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Extra {
    /// Shared contacts.
    Cards(Vec<Card>),
    /// A poll.
    Poll(Poll),
}

/// A shared contact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    /// The name it was shared under.
    pub name: String,
    /// Its WhatsApp number, digits only, when it has one.
    pub phone: Option<String>,
}

impl Card {
    fn of(contact: &wa::message::ContactMessage) -> Self {
        let vcard = contact.vcard.as_deref().unwrap_or_default();
        Self {
            name: contact
                .display_name
                .clone()
                .or_else(|| vcard_field(vcard, "FN"))
                .unwrap_or_default(),
            phone: vcard_phone(vcard),
        }
    }
}

/// A vCard property's value, e.g. `FN:Ana` for `FN`.
fn vcard_field(vcard: &str, name: &str) -> Option<String> {
    vcard.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        let key = key.split(';').next()?;
        key.eq_ignore_ascii_case(name)
            .then(|| value.trim().to_owned())
    })
}

/// The WhatsApp number in a vCard: the `waid` WhatsApp adds to a phone
/// line, else the first phone line's digits.
fn vcard_phone(vcard: &str) -> Option<String> {
    let phones = || {
        vcard.lines().filter(|line| {
            line.split([';', ':'])
                .next()
                // Grouped lines name the property after a dot: `item1.TEL`.
                .and_then(|key| key.rsplit('.').next())
                .is_some_and(|key| key.eq_ignore_ascii_case("TEL"))
        })
    };
    let waid = phones().find_map(|line| {
        let start = line.find("waid=")? + "waid=".len();
        let digits: String = line[start..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        Some(digits).filter(|digits| !digits.is_empty())
    });
    waid.or_else(|| {
        let line = phones().next()?;
        let digits: String = line
            .rsplit(':')
            .next()?
            .chars()
            .filter(char::is_ascii_digit)
            .collect();
        Some(digits).filter(|digits| digits.len() >= 8)
    })
}

/// A poll's question and choices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Poll {
    /// What it asks.
    pub question: String,
    /// The answers to choose from.
    pub options: Vec<String>,
    /// More than one answer may be chosen.
    pub multiple: bool,
}

impl Extra {
    fn of(message: &wa::Message) -> Option<Self> {
        if let Some(contact) = message.contact_message.as_option() {
            return Some(Self::Cards(vec![Card::of(contact)]));
        }
        if let Some(contacts) = message.contacts_array_message.as_option() {
            return Some(Self::Cards(
                contacts.contacts.iter().map(Card::of).collect(),
            ));
        }
        let poll = [
            &message.poll_creation_message,
            &message.poll_creation_message_v2,
            &message.poll_creation_message_v3,
        ]
        .into_iter()
        .find_map(|poll| poll.as_option())?;
        Some(Self::Poll(Poll {
            question: poll.name.clone().unwrap_or_default(),
            options: poll
                .options
                .iter()
                .filter_map(|option| option.option_name.clone())
                .collect(),
            multiple: poll.selectable_options_count.is_none_or(|count| count != 1),
        }))
    }
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
        let outer = message;
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
        } else if message.contact_message.is_set() || message.contacts_array_message.is_set() {
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
            extra: Extra::of(message),
            forwarding: Forwarding::of(message),
            link: LinkPreview::of(message),
            // Wrapped messages may keep it on either layer.
            secret: crate::polls::secret(outer).or_else(|| crate::polls::secret(message)),
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
    fn reads_the_whatsapp_number_of_a_shared_contact() {
        let vcard = "BEGIN:VCARD\nVERSION:3.0\nFN:Ana Souza\nitem1.TEL;waid=5511912345678:+55 11 91234-5678\nEND:VCARD";
        assert_eq!(vcard_phone(vcard).as_deref(), Some("5511912345678"));
        assert_eq!(vcard_field(vcard, "FN").as_deref(), Some("Ana Souza"));
        let plain = "BEGIN:VCARD\nTEL;TYPE=CELL:+1 (555) 010-9999\nEND:VCARD";
        assert_eq!(vcard_phone(plain).as_deref(), Some("15550109999"));
    }

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
