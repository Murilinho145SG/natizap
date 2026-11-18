use chrono::{DateTime, Datelike as _, Local, NaiveDate, TimeZone as _};
use gpui::{Image, SharedString};
use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use natizap_protocol::{ChatId, ContentKind, Extra, Forwarding, Mark, Marked, Reply};
use natizap_ui::name_color;
use natizap_ui::theme::{ACCENT, MUTED};

pub(crate) use crate::authors::Members;
use crate::authors::{Author, Senders};
use crate::media::Fetch;
use crate::message::{Delivery, Message, Reaction};
use crate::poll::{Addons, PollState};
use crate::preview::preview;

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
    pub(crate) author: Option<Author>,
    /// Forwarded, and how often.
    pub(crate) forwarding: Option<Forwarding>,
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
    pub(crate) sender: Option<Author>,
    pub(crate) time: SharedString,
    pub(crate) delivery: Delivery,
    /// Starred by this account.
    pub(crate) starred: bool,
    /// Who wrote it, as message keys name them: only in groups, and only
    /// for others' messages.
    pub(crate) participant: Option<ChatId>,
    /// The text as written, markup included, for forwarding.
    pub(crate) source_text: Option<String>,
    /// Starts a new burst, so it gets more space above it.
    pub(crate) starts_burst: bool,
    /// The votes, on a poll.
    pub(crate) poll: Option<PollState>,
}

/// What a bubble shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Body {
    /// The message was deleted for everyone.
    Deleted,
    /// A call nobody answered; natizap cannot take calls yet.
    MissedCall {
        /// It was a video call.
        video: bool,
    },
    /// The message's content.
    Content {
        /// What the media is, for non-text messages.
        media: Option<&'static str>,
        /// Text or caption, with its markup applied.
        text: Option<Text>,
        /// Shared contacts or a poll.
        extra: Option<Extra>,
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
    /// A document's name.
    pub(crate) name: Option<SharedString>,
    /// Duration, or a document's pages, type and size.
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
    /// The quoted message's id, to go to it.
    pub(crate) id: SharedString,
    pub(crate) author: SharedString,
    /// The author's number beside a chosen name, in groups.
    pub(crate) phone: Option<SharedString>,
    pub(crate) color: u32,
    pub(crate) text: SharedString,
    /// What the quoted message carries, for its icon.
    pub(crate) kind: ContentKind,
    /// The quoted photo's or video's preview, shown beside the text.
    pub(crate) thumbnail: Option<Arc<Image>>,
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
/// `contact` names the other side of a one-to-one chat, whose messages
/// carry no sender of their own.
pub(crate) fn items(
    messages: &[Message],
    addons: &Addons<'_>,
    contact: &str,
    members: &Members,
    now: DateTime<Local>,
) -> Vec<Item> {
    let senders = Senders::of(messages, members);
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
        let mut bubble = bubble(message, sent, starts_burst, &by_id, contact, &senders);
        bubble.poll = PollState::of(
            message,
            addons.votes.get(&message.id).map_or(&[], Vec::as_slice),
        );
        if let Some(reactions) = addons.reactions.get(&message.id) {
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
    contact: &str,
    senders: &Senders<'_>,
) -> Bubble {
    let sender = message
        .sender
        .as_ref()
        .map(|sender| Author::of(message, sender, senders));
    let author = sender.clone().filter(|_| starts_burst);
    let quote = quote_preview(message, by_id, contact, senders);
    Bubble {
        id: message.id.clone().into(),
        from_me: message.from_me,
        author,
        forwarding: message.forwarding,
        body: if message.deleted {
            Body::Deleted
        } else if let ContentKind::MissedVoiceCall | ContentKind::MissedVideoCall = message.kind {
            Body::MissedCall {
                video: message.kind == ContentKind::MissedVideoCall,
            }
        } else {
            Body::Content {
                // Downloadable attachments render themselves; the rest
                // (locations, contacts, polls) are named.
                media: (message.kind != ContentKind::Text
                    && message.media.is_none()
                    && message.extra.is_none())
                .then(|| message.kind.label()),
                text: message.text.as_deref().map(Text::parse),
                extra: message.extra.clone(),
                edited: message.edited,
            }
        },
        media: media_view(message),
        quote,
        reactions: Vec::new(),
        my_reaction: None,
        poll: None,
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
        starred: message.starred,
        participant: message.sender.clone().filter(|_| !message.from_me),
        source_text: message.text.clone(),
        starts_burst,
    }
}

/// Largest box a photo or video preview may take.
const MEDIA_BOX: (f32, f32) = (300.0, 340.0);
/// Stickers render at a fixed size, like on the phone.
const STICKER_SIZE: f32 = 150.0;

/// The block at the top of a reply: the quoted message as loaded, else
/// what the reply carried of it.
fn quote_preview(
    message: &Message,
    by_id: &HashMap<&str, &Message>,
    contact: &str,
    senders: &Senders<'_>,
) -> Option<QuotePreview> {
    message
        .quote
        .as_ref()
        .map(|quote| match by_id.get(quote.id.as_str()) {
            Some(quoted) => {
                let (author, color, phone) = match (&quoted.sender, quoted.from_me) {
                    (_, true) => ("You".into(), ACCENT, None),
                    (Some(sender), false) => {
                        let author = Author::of(quoted, sender, senders);
                        (author.name, author.color, author.phone)
                    }
                    (None, false) => (
                        contact.to_owned().into(),
                        name_color(quoted.chat.as_str()),
                        None,
                    ),
                };
                QuotePreview {
                    id: quote.id.clone().into(),
                    author,
                    phone,
                    color,
                    text: quote_text(quoted).into(),
                    kind: quoted.kind,
                    thumbnail: quoted
                        .media
                        .as_ref()
                        .and_then(|media| media.thumbnail.clone())
                        .filter(|_| {
                            matches!(
                                quoted.kind,
                                ContentKind::Photo | ContentKind::Video | ContentKind::Sticker
                            )
                        })
                        .map(|bytes| preview(quoted.kind, bytes)),
                }
            }
            // The quoted message is older than what is loaded, or was never
            // synced: fall back to what the reply carried.
            None => QuotePreview {
                id: quote.id.clone().into(),
                author: quote
                    .sender
                    .as_ref()
                    .and_then(ChatId::phone_number)
                    .unwrap_or_else(|| "Message".to_owned())
                    .into(),
                phone: None,
                color: MUTED,
                text: quote
                    .summary
                    .as_deref()
                    .map_or_default(|text| Marked::parse(text).text)
                    .into(),
                kind: ContentKind::Text,
                thumbnail: None,
            },
        })
}

#[expect(
    clippy::cast_precision_loss,
    reason = "image sides are far below f32's exact range"
)]
pub(crate) fn media_view(message: &Message) -> Option<MediaView> {
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
            [
                media.pages.map(|pages| match pages {
                    1 => "1 page".to_owned(),
                    pages => format!("{pages} pages"),
                }),
                media
                    .file_name
                    .as_deref()
                    .and_then(|name| name.rsplit_once('.'))
                    .map(|(_, extension)| extension.to_uppercase()),
                media.size.map(file_size),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · "),
        ),
        _ => None,
    };
    let file = message.media_file.clone();
    let poster = message
        .media_poster
        .clone()
        .filter(|poster| poster.exists());
    // Softening costs a decode and an encode; skip it when the file or a
    // still shows instead.
    let shown_instead = poster.is_some()
        || matches!(message.kind, ContentKind::Photo | ContentKind::Sticker)
            && file.as_deref().is_some_and(Path::exists);
    let preview = media
        .thumbnail
        .clone()
        .filter(|_| !shown_instead)
        .map(|bytes| preview(message.kind, bytes));
    Some(MediaView {
        file,
        poster,
        preview,
        size,
        name: media.file_name.clone().map(Into::into),
        detail: detail.map(Into::into),
        fetch,
    })
}

#[expect(
    clippy::cast_precision_loss,
    reason = "a rounded size needs no exact bytes"
)]
pub(crate) fn file_size(bytes: u64) -> String {
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

/// The quoted message's line: its caption or text, a voice note's length,
/// or else what it is. The icon beside it already says it is a photo, so a
/// caption stands alone.
fn quote_text(message: &Message) -> String {
    let caption = message.text.as_deref().map(|text| Marked::parse(text).text);
    let length = message
        .media
        .as_ref()
        .and_then(|media| media.seconds)
        .map(|seconds| format!("{}:{:02}", seconds / 60, seconds % 60));
    match (caption, message.kind) {
        (Some(text), _) => text,
        (None, ContentKind::VoiceMessage | ContentKind::Audio) => {
            length.unwrap_or_else(|| message.kind.label().to_owned())
        }
        (None, kind) => kind.label().to_owned(),
    }
}

/// One plain line describing a message.
pub(crate) fn summary(message: &Message) -> String {
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
    use natizap_protocol::PersonName;

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
            starred: false,
            sender_phone: None,
            extra: None,
            forwarding: None,
            link: None,
            secret: None,
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
                    let author = bubble
                        .author
                        .as_ref()
                        .map_or("", |author| author.name.as_ref());
                    format!("{burst}{side}{author}")
                }
            })
            .collect()
    }

    #[test]
    fn names_a_member_as_the_address_book_or_they_did() {
        let mut saved = message(1_000, false, Some("309@lid"));
        saved.author = Some("brunao".into());
        let mut chosen = message(9_000, false, Some("410@lid"));
        chosen.author = None;
        let phone = ChatId::new("5511912345678@s.whatsapp.net");
        let members = Members {
            phones: HashMap::from([(ChatId::new("410@lid"), phone.clone())]),
            names: Arc::new(HashMap::from([
                (
                    ChatId::new("309@lid"),
                    PersonName {
                        saved: Some("Bruno Lima".into()),
                        chosen: None,
                    },
                ),
                (
                    phone,
                    PersonName {
                        saved: None,
                        chosen: Some("Carla".into()),
                    },
                ),
            ])),
            ..Members::default()
        };
        let names: Vec<String> = items(
            &[saved, chosen],
            &Addons {
                reactions: &HashMap::new(),
                votes: &HashMap::new(),
            },
            "",
            &members,
            Local::now(),
        )
        .into_iter()
        .filter_map(|item| match item {
            Item::Bubble(bubble) => bubble.sender.map(|sender| sender.name.to_string()),
            Item::Day(_) => None,
        })
        .collect();
        assert_eq!(names, ["Bruno Lima", "~ Carla"]);
    }

    #[test]
    fn names_a_member_from_their_other_messages_and_the_groups_numbers() {
        let lid = ChatId::new("208@lid");
        let mut named = message(1_000, false, Some("208@lid"));
        named.author = Some("Beatriz".into());
        let mut unnamed = message(5_000, false, Some("208@lid"));
        unnamed.author = None;
        let members = Members {
            community_admins: vec![ChatId::new("5511976543210@s.whatsapp.net")],
            phones: HashMap::from([(lid, ChatId::new("5511976543210@s.whatsapp.net"))]),
            ..Members::default()
        };
        let now = Local::now();
        let authors: Vec<(String, Option<String>, Option<String>)> = items(
            &[named, unnamed],
            &Addons {
                reactions: &HashMap::new(),
                votes: &HashMap::new(),
            },
            "",
            &members,
            now,
        )
        .into_iter()
        .filter_map(|item| match item {
            Item::Bubble(bubble) => bubble.sender,
            Item::Day(_) => None,
        })
        .map(|author| {
            (
                author.name.to_string(),
                author.phone.map(|phone| phone.to_string()),
                author.role.map(|role| role.to_string()),
            )
        })
        .collect();
        let expected = (
            "~ Beatriz".to_owned(),
            Some("+55 11 97654-3210".to_owned()),
            Some("Community admin".to_owned()),
        );
        assert_eq!(authors, [expected.clone(), expected]);
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
            layout(&items(
                &messages,
                &Addons {
                    reactions: &HashMap::new(),
                    votes: &HashMap::new(),
                },
                "Ana",
                &Members::default(),
                now
            )),
            [
                "[Yesterday]",
                "+them~ Ana",
                "[Today]",
                "+them~ Ana",
                "them",
                "+me",
                "+me"
            ]
        );
    }
}
