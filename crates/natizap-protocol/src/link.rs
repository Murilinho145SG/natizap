//! The preview a phone attaches to a message with a link: the page's
//! title, description and picture, fetched by the sender's phone.

use whatsapp_rust::prelude::wa;

/// A link's preview, as the sender's phone made it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkPreview {
    /// The link the preview is of, as written in the message.
    pub url: Option<String>,
    /// The page's title.
    pub title: Option<String>,
    /// The page's description.
    pub description: Option<String>,
    /// A small JPEG of the page's picture.
    pub thumbnail: Option<Vec<u8>>,
}

impl LinkPreview {
    /// The preview in `message`, when it has one worth showing: one with a
    /// title or a picture, not only the link again.
    pub(crate) fn of(message: &wa::Message) -> Option<Self> {
        let text = message.extended_text_message.as_option()?;
        let filled =
            |field: &Option<String>| field.clone().filter(|value| !value.trim().is_empty());
        let preview = Self {
            url: filled(&text.matched_text),
            title: filled(&text.title),
            description: filled(&text.description),
            thumbnail: text
                .jpeg_thumbnail
                .clone()
                .filter(|bytes| !bytes.is_empty()),
        };
        (preview.title.is_some() || preview.thumbnail.is_some()).then_some(preview)
    }
}

#[cfg(test)]
mod tests {
    use whatsapp_rust::prelude::wa;

    use super::LinkPreview;

    fn message(title: Option<&str>, thumbnail: Option<Vec<u8>>) -> wa::Message {
        wa::Message {
            extended_text_message: wa::message::ExtendedTextMessage {
                text: Some("see https://natizap.dev".into()),
                matched_text: Some("https://natizap.dev".into()),
                title: title.map(Into::into),
                description: Some("A WhatsApp client".into()),
                jpeg_thumbnail: thumbnail,
                ..Default::default()
            }
            .into(),
            ..Default::default()
        }
    }

    #[test]
    fn reads_the_preview_the_sender_attached() {
        let preview = LinkPreview::of(&message(Some("natizap"), Some(vec![0xff, 0xd8])));
        assert_eq!(
            preview,
            Some(LinkPreview {
                url: Some("https://natizap.dev".into()),
                title: Some("natizap".into()),
                description: Some("A WhatsApp client".into()),
                thumbnail: Some(vec![0xff, 0xd8]),
            })
        );
    }

    #[test]
    fn has_none_without_a_title_or_picture() {
        assert_eq!(LinkPreview::of(&message(Some("  "), None)), None);
        assert_eq!(LinkPreview::of(&wa::Message::default()), None);
    }
}
