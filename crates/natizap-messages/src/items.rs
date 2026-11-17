use chrono::{DateTime, Datelike as _, Local, NaiveDate, TimeZone as _};
use gpui::{Image, ImageFormat, SharedString};
use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

use natizap_protocol::{ChatId, ContentKind, Mark, Marked, Reply};
use natizap_ui::name_color;
use natizap_ui::theme::{ACCENT, MUTED};

use crate::media::Fetch;
use crate::message::{Delivery, Message, Reaction};

/// Messages from the same side closer than this read as one burst.
const BURST_GAP_SECONDS: i64 = 5 * 60;

/// One row of a conversation.
#[derive(Debug, Clone)]
pub(crate) enum Item {
    /// Separates days: "Today", "Yesterday", a weekday or a date.
    Day(SharedString),
    /// A message.
    Bubble(Box<Bubble>),
}

/// A message, ready to render.
#[derive(Debug, Clone)]
pub(crate) struct Bubble {
    pub(crate) id: SharedString,
    pub(crate) from_me: bool,
    /// Sender name and its color; only on the first bubble of a burst in a
    /// group.
    pub(crate) author: Option<(SharedString, u32)>,
    pub(crate) body: Body,
    /// The attachment, shown above the text.
    pub(crate) media: Option<MediaView>,
    /// The message it replies to.
    pub(crate) quote: Option<QuotePreview>,
    /// What to quote when replying to it.
    pub(crate) reply: Reply,
    /// Reactions, most common first.
    pub(crate) reactions: Vec<ReactionChip>,
    /// The emoji this account reacted with, if any.
    pub(crate) my_reaction: Option<String>,
    /// Who wrote it and their color, in groups; shown when replying.
    pub(crate) sender: Option<(SharedString, u32)>,
    pub(crate) time: SharedString,
    pub(crate) delivery: Delivery,
    /// Starts a new burst, so it gets more space above it.
    pub(crate) starts_burst: bool,
}

/// What a bubble shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Body {
    /// The message was deleted for everyone.
    Deleted,
    /// The message's content.
    Content {
        /// What the media is, for non-text messages.
        media: Option<&'static str>,
        /// Text or caption, with its markup applied.
        text: Option<Text>,
        /// Changed after sending.
        edited: bool,
    },
}

/// An attachment, ready to render: a preview at display size and what to
/// fetch or open on click.
#[derive(Debug, Clone)]
pub(crate) struct MediaView {
    pub(crate) fetch: Fetch,
    /// The downloaded file, once there is one.
    pub(crate) file: Option<PathBuf>,
    /// A still of a downloaded video.
    pub(crate) poster: Option<PathBuf>,
    /// The inline preview, shown until the file is downloaded.
    pub(crate) preview: Option<Arc<Image>>,
    /// Display size in pixels, for photos, videos and stickers.
    pub(crate) size: (f32, f32),
    /// Duration, or a document's name and size.
    pub(crate) detail: Option<SharedString>,
}

/// One emoji under a bubble and how many people reacted with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReactionChip {
    pub(crate) emoji: SharedString,
    pub(crate) count: usize,
}

/// The quoted message shown at the top of a reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct QuotePreview {
    pub(crate) author: SharedString,
    pub(crate) color: u32,
    pub(crate) text: SharedString,
}

/// Message text with its markup read: what to show and how to style it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Text {
    pub(crate) text: SharedString,
    pub(crate) marks: Arc<[(Range<usize>, Mark)]>,
}

impl Text {
    fn parse(source: &str) -> Self {
        let marked = Marked::parse(source);
        Self {
            text: marked.text.into(),
            marks: marked.marks.into(),
        }
    }
}

/// Lays out `messages` (oldest first) as rows, with day separators.
pub(crate) fn items(
    messages: &[Message],
    reactions: &HashMap<String, Vec<Reaction>>,
    now: DateTime<Local>,
) -> Vec<Item> {
    let by_id: HashMap<&str, &Message> = messages
        .iter()
        .map(|message| (message.id.as_str(), message))
        .collect();
    let mut items = Vec::with_capacity(messages.len() + 8);
    let mut previous: Option<(&Message, NaiveDate)> = None;
    for message in messages {
        let Some(sent) = Local.timestamp_opt(message.sent_at, 0).single() else {
            continue;
        };
        let day = sent.date_naive();
        let new_day = previous.is_none_or(|(_, previous_day)| previous_day != day);
        if new_day {
            items.push(Item::Day(day_label(day, now).into()));
        }
        let starts_burst = new_day
            || previous.is_none_or(|(previous, _)| {
                previous.from_me != message.from_me
                    || previous.sender != message.sender
                    || message.sent_at - previous.sent_at > BURST_GAP_SECONDS
            });
        let mut bubble = bubble(message, sent, starts_burst, &by_id);
        if let Some(reactions) = reactions.get(&message.id) {
            bubble.my_reaction = reactions
                .iter()
                .find(|reaction| reaction.mine)
                .map(|r| r.emoji.clone());
            bubble.reactions = chips(reactions);
        }
        items.push(Item::Bubble(Box::new(bubble)));
        previous = Some((message, day));
    }
    items
}

fn bubble(
    message: &Message,
    sent: DateTime<Local>,
    starts_burst: bool,
    by_id: &HashMap<&str, &Message>,
) -> Bubble {
    let sender: Option<(SharedString, u32)> = message.sender.as_ref().map(|sender| {
        (
            sender_name(message, sender).into(),
            name_color(sender.as_str()),
        )
    });
    let author = sender.clone().filter(|_| starts_burst);
    let quote = message
        .quote
        .as_ref()
        .map(|quote| match by_id.get(quote.id.as_str()) {
            Some(quoted) => {
                let (author, color) = if quoted.from_me {
                    ("You".to_owned(), ACCENT)
                } else {
                    let sender = quoted.sender.as_ref().unwrap_or(&quoted.chat);
                    (sender_name(quoted, sender), name_color(sender.as_str()))
                };
                QuotePreview {
                    author: author.into(),
                    color,
                    text: summary(quoted).into(),
                }
            }
            // The quoted message is older than what is loaded, or was never
            // synced: fall back to what the reply carried.
            None => QuotePreview {
                author: quote
                    .sender
                    .as_ref()
                    .and_then(ChatId::phone_number)
                    .unwrap_or_else(|| "Message".to_owned())
                    .into(),
                color: MUTED,
                text: quote
                    .summary
                    .as_deref()
                    .map_or_default(|text| Marked::parse(text).text)
                    .into(),
            },
        });
    Bubble {
        id: message.id.clone().into(),
        from_me: message.from_me,
        author,
        body: if message.deleted {
            Body::Deleted
        } else {
            Body::Content {
                // Downloadable attachments render themselves; the rest
                // (locations, contacts, polls) are named.
                media: (message.kind != ContentKind::Text && message.media.is_none())
                    .then(|| message.kind.label()),
                text: message.text.as_deref().map(Text::parse),
                edited: message.edited,
            }
        },
        media: media_view(message),
        quote,
        reactions: Vec::new(),
        my_reaction: None,
        sender,
        reply: Reply {
            id: message.id.clone(),
            // In 1:1 chats only the other person can be the sender.
            sender: (!message.from_me).then(|| {
                message
                    .sender
                    .clone()
                    .unwrap_or_else(|| message.chat.clone())
            }),
            text: summary(message),
        },
        time: sent.format("%H:%M").to_string().into(),
        delivery: message.delivery,
        starts_burst,
    }
}

/// Largest box a photo or video preview may take.
const MEDIA_BOX: (f32, f32) = (300.0, 340.0);
/// Stickers render at a fixed size, like on the phone.
const STICKER_SIZE: f32 = 150.0;

#[expect(
    clippy::cast_precision_loss,
    reason = "image sides are far below f32's exact range"
)]
fn media_view(message: &Message) -> Option<MediaView> {
    let fetch = Fetch::of(message)?;
    let media = &fetch.media;
    let size = match (message.kind, media.width, media.height) {
        (ContentKind::Sticker, ..) => (STICKER_SIZE, STICKER_SIZE),
        (_, Some(width), Some(height)) if width > 0 && height > 0 => {
            let (width, height) = (width as f32, height as f32);
            let scale = (MEDIA_BOX.0 / width).min(MEDIA_BOX.1 / height).min(1.0);
            ((width * scale).max(120.0), (height * scale).max(80.0))
        }
        _ => (260.0, 200.0),
    };
    let detail = match message.kind {
        ContentKind::Video | ContentKind::VoiceMessage | ContentKind::Audio => media
            .seconds
            .map(|seconds| format!("{}:{:02}", seconds / 60, seconds % 60)),
        ContentKind::Document => Some(
            [media.file_name.clone(), media.size.map(file_size)]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" · "),
        ),
        _ => None,
    };
    let preview = media.thumbnail.clone().map(|bytes| {
        let format = if bytes.starts_with(b"\x89PNG") {
            ImageFormat::Png
        } else {
            ImageFormat::Jpeg
        };
        Arc::new(Image::from_bytes(format, bytes))
    });
    Some(MediaView {
        file: message.media_file.clone(),
        poster: message
            .media_poster
            .clone()
            .filter(|poster| poster.exists()),
        preview,
        size,
        detail: detail.map(Into::into),
        fetch,
    })
}

#[expect(
    clippy::cast_precision_loss,
    reason = "a rounded size needs no exact bytes"
)]
fn file_size(bytes: u64) -> String {
    match bytes {
        0..1_000 => format!("{bytes} B"),
        1_000..1_000_000 => format!("{:.0} kB", bytes as f64 / 1e3),
        1_000_000..1_000_000_000 => format!("{:.1} MB", bytes as f64 / 1e6),
        _ => format!("{:.1} GB", bytes as f64 / 1e9),
    }
}

fn chips(reactions: &[Reaction]) -> Vec<ReactionChip> {
    let mut counts: Vec<(&str, usize)> = Vec::new();
    for reaction in reactions {
        match counts
            .iter_mut()
            .find(|(emoji, _)| *emoji == reaction.emoji)
        {
            Some((_, count)) => *count += 1,
            None => counts.push((&reaction.emoji, 1)),
        }
    }
    counts.sort_by_key(|&(_, count)| std::cmp::Reverse(count));
    counts
        .into_iter()
        .map(|(emoji, count)| ReactionChip {
            emoji: emoji.to_owned().into(),
            count,
        })
        .collect()
}

fn sender_name(message: &Message, sender: &ChatId) -> String {
    message
        .author
        .clone()
        .or_else(|| sender.phone_number())
        .unwrap_or_else(|| sender.as_str().to_owned())
}

/// One plain line describing a message.
fn summary(message: &Message) -> String {
    let text = message.text.as_deref().map(|text| Marked::parse(text).text);
    match (text, message.kind) {
        (Some(text), ContentKind::Text) => text,
        (Some(caption), kind) => format!("{}: {caption}", kind.label()),
        (None, kind) => kind.label().to_owned(),
    }
}

fn day_label(day: NaiveDate, now: DateTime<Local>) -> String {
    match (now.date_naive() - day).num_days() {
        ..=0 => "Today".to_owned(),
        1 => "Yesterday".to_owned(),
        2..=6 => day.format("%A").to_string(),
        _ if day.year() == now.year() => day.format("%-d %B").to_string(),
        _ => day.format("%-d %B %Y").to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(day: u32, hour: u32, minute: u32) -> i64 {
        Local
            .with_ymd_and_hms(2026, 11, day, hour, minute, 0)
            .single()
            .map_or(0, |time| time.timestamp())
    }

    fn message(sent_at: i64, from_me: bool, sender: Option<&str>) -> Message {
        Message {
            chat: ChatId::new("1@g.us"),
            id: sent_at.to_string(),
            from_me,
            sender: sender.map(ChatId::new),
            author: sender.map(|_| "Ana".to_owned()),
            sent_at,
            kind: ContentKind::Text,
            text: Some("hi".into()),
            edited: false,
            deleted: false,
            delivery: Delivery::Sent,
            quote: None,
            media: None,
            media_file: None,
            media_poster: None,
        }
    }

    fn layout(items: &[Item]) -> Vec<String> {
        items
            .iter()
            .map(|item| match item {
                Item::Day(label) => format!("[{label}]"),
                Item::Bubble(bubble) => {
                    let side = if bubble.from_me { "me" } else { "them" };
                    let burst = if bubble.starts_burst { "+" } else { "" };
                    let author = bubble.author.as_ref().map_or("", |(name, _)| name.as_ref());
                    format!("{burst}{side}{author}")
                }
            })
            .collect()
    }

    #[test]
    fn separates_days_and_groups_bursts_by_sender_and_time() {
        let Some(now) = Local.with_ymd_and_hms(2026, 11, 16, 18, 0, 0).single() else {
            panic!("test date must exist in the local zone");
        };
        let ana = Some("5511@s.whatsapp.net");
        let messages = [
            message(at(15, 9, 0), false, ana),
            message(at(16, 10, 0), false, ana),
            message(at(16, 10, 1), false, ana),
            message(at(16, 10, 2), true, None),
            message(at(16, 10, 30), true, None),
        ];

        assert_eq!(
            layout(&items(&messages, &HashMap::new(), now)),
            [
                "[Yesterday]",
                "+themAna",
                "[Today]",
                "+themAna",
                "them",
                "+me",
                "+me"
            ]
        );
    }
}
