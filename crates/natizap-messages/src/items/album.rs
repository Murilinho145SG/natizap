//! Photos and videos sent one after another, folded into one bubble as the
//! phone does, so a burst of pictures does not flood the chat.

use std::ops::Range;

use chrono::{DateTime, Local, TimeZone as _};
use natizap_protocol::ContentKind;

use super::{Body, Bubble, Senders, Text, media_view};
use crate::message::Message;

/// At least this many in a row make an album, as on the phone.
const ALBUM_MIN: usize = 4;
/// Pictures sent further apart than this stay apart.
const ALBUM_GAP_SECONDS: i64 = 60;

/// The runs of `messages` (oldest first) to fold into one bubble each, by
/// index. Only the last picture of a run may carry a caption: a caption
/// ends the run, and the album shows it under the pictures.
pub(super) fn albums(messages: &[Message]) -> Vec<Range<usize>> {
    let mut runs = Vec::new();
    let mut start = 0;
    while start < messages.len() {
        if !foldable(&messages[start]) {
            start += 1;
            continue;
        }
        let mut end = start + 1;
        while end < messages.len()
            && messages[end - 1].text.is_none()
            && foldable(&messages[end])
            && follows(&messages[end - 1], &messages[end])
        {
            end += 1;
        }
        if end - start >= ALBUM_MIN {
            runs.push(start..end);
        }
        start = end;
    }
    runs
}

/// Adds `message`, sent at `sent`, to the album `bubble` heads: its
/// picture joins the grid, and the album takes its time, its ticks and,
/// on the last picture, its caption.
pub(super) fn fold(
    bubble: &mut Bubble,
    message: &Message,
    sent: DateTime<Local>,
    senders: &Senders<'_>,
) {
    bubble.album.extend(media_view(message));
    bubble.time = sent.format("%H:%M").to_string().into();
    bubble.delivery = message.delivery;
    if let (Some(text), Body::Content { text: caption, .. }) = (&message.text, &mut bubble.body) {
        *caption = Some(Text::parse(text, senders));
        bubble.source_text = Some(text.clone());
    }
}

/// A photo or video that shows as one, on its own: not deleted, and not a
/// reply, whose quote would have nowhere to go in a grid.
fn foldable(message: &Message) -> bool {
    matches!(message.kind, ContentKind::Photo | ContentKind::Video)
        && message.media.is_some()
        && !message.deleted
        && message.quote.is_none()
}

/// Whether `next` was sent right after `previous`, by the same person, on
/// the same day.
fn follows(previous: &Message, next: &Message) -> bool {
    let day = |message: &Message| {
        Local
            .timestamp_opt(message.sent_at, 0)
            .single()
            .map(|sent| sent.date_naive())
    };
    previous.from_me == next.from_me
        && previous.sender == next.sender
        && (0..=ALBUM_GAP_SECONDS).contains(&(next.sent_at - previous.sent_at))
        && day(previous) == day(next)
}
