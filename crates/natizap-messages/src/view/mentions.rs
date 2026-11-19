//! Mentioning someone in a group: typing `@` offers the members, picking
//! one writes their name, and sending turns each name picked into the
//! `@` and digits WhatsApp reads, naming them in the message.

use std::ops::Range;

use gpui::prelude::*;
use gpui::{
    Context, Focusable as _, KeyDownEvent, ScrollStrategy, SharedString, UniformListScrollHandle,
    Window, deferred, div, px, relative, rgb, uniform_list,
};
use natizap_protocol::ChatId;
use natizap_ui::motion::rise_in;
use natizap_ui::text_input::{Down, Submit, Up};
use natizap_ui::theme::ActiveTheme as _;
use natizap_ui::{avatar, scrollbar};

use super::ConversationView;
use crate::log::Mentionable;

/// How many members the list shows before it scrolls.
const SHOWN: usize = 6;
/// Height of a member's row.
const ROW: f32 = 44.0;
/// Size of a member's picture in the list.
const PICTURE: f32 = 30.0;

/// Mentions being written in the composer.
#[derive(Debug, Default)]
pub(super) struct Mentions {
    /// Where the `@` being typed after is, as a byte offset, and what
    /// follows it up to the caret.
    typing: Option<(usize, String)>,
    /// The member lit in the list.
    lit: usize,
    /// The `@` whose list was closed with Escape: it stays closed.
    dismissed: Option<usize>,
    /// Names picked so far, as written, and whom each names.
    picked: Vec<(String, ChatId)>,
    /// How far the list is scrolled.
    scroll: UniformListScrollHandle,
}

impl ConversationView {
    /// Follows what is typed: an `@` at the start of a word, with no space
    /// between it and the caret, opens the list of members.
    pub(super) fn follow_mention(&mut self, cx: &mut Context<Self>) {
        let composer = self.composer.read(cx);
        let (text, caret) = (composer.text(), composer.cursor());
        let typing = typing_mention(text, caret);
        let mentions = &mut self.mentions;
        if mentions.typing.as_ref().map(|(_, query)| query) != typing.as_ref().map(|(_, q)| q) {
            mentions.lit = 0;
        }
        if typing.as_ref().map(|&(at, _)| at) != mentions.dismissed {
            mentions.dismissed = None;
        }
        // A name picked and then edited away no longer names anyone.
        mentions
            .picked
            .retain(|(name, _)| text.contains(name.as_str()));
        mentions.typing = typing;
        let highlights = mentioned_ranges(text, &mentions.picked);
        // Only redraws when they changed, so this observer is not woken
        // again by its own update.
        self.composer
            .update(cx, |composer, cx| composer.set_highlights(highlights, cx));
    }

    /// The members matching what follows the `@`, while the list is open.
    fn candidates(&self, cx: &Context<Self>) -> Vec<Mentionable> {
        let Some((at, query)) = &self.mentions.typing else {
            return Vec::new();
        };
        if self.mentions.dismissed == Some(*at) {
            return Vec::new();
        }
        let query = query.to_lowercase();
        let digits: String = query.chars().filter(char::is_ascii_digit).collect();
        self.log
            .read(cx)
            .mentionable(cx)
            .into_iter()
            .filter(|member| {
                member.name.to_lowercase().contains(&query)
                    || !digits.is_empty()
                        && member.phone.as_ref().is_some_and(|phone| {
                            phone
                                .chars()
                                .filter(char::is_ascii_digit)
                                .collect::<String>()
                                .contains(&digits)
                        })
            })
            .collect()
    }

    /// The composer's keys while the list is open: up and down light a
    /// member, Enter and Tab pick them, Escape closes the list. Arrows and
    /// Enter are the field's own actions, which run before any key
    /// listener, so they are caught on their way down to it.
    pub(super) fn with_mention_keys(
        composer: gpui::Stateful<gpui::Div>,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        composer
            .capture_action(cx.listener(|this, _: &Up, window, cx| {
                this.mention_step(MentionStep::Up, window, cx);
            }))
            .capture_action(cx.listener(|this, _: &Down, window, cx| {
                this.mention_step(MentionStep::Down, window, cx);
            }))
            .capture_action(cx.listener(|this, _: &Submit, window, cx| {
                this.mention_step(MentionStep::Pick, window, cx);
            }))
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                let step = match event.keystroke.key.as_str() {
                    "tab" if !event.keystroke.modifiers.shift => MentionStep::Pick,
                    "escape" => MentionStep::Close,
                    _ => return,
                };
                this.mention_step(step, window, cx);
            }))
    }

    /// Takes `step` in the open list; with the list closed, the key goes
    /// on to the field.
    fn mention_step(&mut self, step: MentionStep, window: &mut Window, cx: &mut Context<Self>) {
        let candidates = self.candidates(cx);
        let Some(last) = candidates.len().checked_sub(1) else {
            return;
        };
        let lit = self.mentions.lit.min(last);
        match step {
            MentionStep::Up => self.mentions.lit = lit.checked_sub(1).unwrap_or(last),
            MentionStep::Down => self.mentions.lit = if lit >= last { 0 } else { lit + 1 },
            MentionStep::Pick => self.mention(&candidates[lit].clone(), window, cx),
            MentionStep::Close => {
                self.mentions.dismissed = self.mentions.typing.as_ref().map(|&(at, _)| at);
            }
        }
        self.mentions
            .scroll
            .scroll_to_item(self.mentions.lit, ScrollStrategy::Nearest);
        cx.stop_propagation();
        cx.notify();
    }

    /// Writes `member`'s name in place of the `@` being typed.
    fn mention(&mut self, member: &Mentionable, window: &mut Window, cx: &mut Context<Self>) {
        let Some((at, _)) = self.mentions.typing.take() else {
            return;
        };
        let name = format!("@{}", member.name.trim_start_matches("~ "));
        self.composer.update(cx, |composer, cx| {
            let caret = composer.cursor();
            composer.replace(at..caret, &format!("{name} "), window, cx);
        });
        self.mentions.picked.push((name, member.address.clone()));
        cx.notify();
    }

    /// `text` to send, each name picked written as WhatsApp reads it, and
    /// whom it mentions; forgets the names picked.
    pub(super) fn take_mentions(&mut self, text: &str) -> (String, Vec<ChatId>) {
        let picked = std::mem::take(&mut self.mentions).picked;
        written_mentions(text, picked)
    }

    /// The members to mention, over the composer, while an `@` is typed:
    /// a few at once, the rest a scroll away.
    pub(super) fn mention_list(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let theme = *cx.theme();
        let candidates = self.candidates(cx);
        if candidates.is_empty() {
            return None;
        }
        let count = candidates.len();
        let lit = self.mentions.lit.min(count - 1);
        let rows = uniform_list(
            "mentions",
            count,
            cx.processor(move |this, range: Range<usize>, _, cx| {
                candidates[range.clone()]
                    .iter()
                    .zip(range)
                    .map(|(member, index)| {
                        let picture = this
                            .pictures
                            .update(cx, |pictures, cx| pictures.picture(&member.address, cx));
                        let chosen = member.clone();
                        div().px_1().child(
                            div()
                                .id(("mention", index))
                                .h(px(ROW))
                                .px_3()
                                .flex()
                                .items_center()
                                .gap_3()
                                .rounded_md()
                                .cursor_pointer()
                                .when(index == lit, |row| row.bg(rgb(theme.raised)))
                                .hover(|row| row.bg(rgb(theme.raised)))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.mention(&chosen, window, cx);
                                    window.focus(&this.composer.focus_handle(cx), cx);
                                }))
                                .child(avatar(
                                    member.address.as_str(),
                                    &member.name,
                                    picture,
                                    px(PICTURE),
                                ))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .text_size(px(14.0))
                                        .child(member.name.clone()),
                                )
                                .children(member.phone.clone().map(|phone| {
                                    div()
                                        .flex_none()
                                        .text_size(px(12.5))
                                        .text_color(rgb(theme.muted))
                                        .child(phone)
                                })),
                        )
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.mentions.scroll)
        .size_full();
        #[expect(clippy::cast_precision_loss, reason = "at most a handful of rows")]
        let height = ROW * count.min(SHOWN) as f32;
        let scrolled = div().relative().h(px(height)).child(rows).child(scrollbar(
            "mentions-scrollbar",
            &self.mentions.scroll,
            window,
            cx,
        ));
        let shown = SharedString::from(format!("mentions-{}", count.min(SHOWN)));
        Some(deferred(
            div()
                .absolute()
                .bottom(relative(1.0))
                .left_4()
                .right_4()
                .mb_2()
                .occlude()
                .child(rise_in(
                    shown,
                    div()
                        .max_w(px(420.0))
                        .py_1()
                        .rounded_xl()
                        .bg(rgb(theme.panel))
                        .map(|list| theme.glass(list))
                        .child(scrolled),
                    6.0,
                )),
        ))
    }
}

/// A key's meaning while the list of members is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MentionStep {
    Up,
    Down,
    Pick,
    Close,
}

/// The `@` being typed after in `text`, as a byte offset, and what follows
/// it up to `caret`: it starts a word, and no space comes after it.
fn typing_mention(text: &str, caret: usize) -> Option<(usize, String)> {
    let before = text.get(..caret)?;
    let at = before.rfind('@')?;
    let word_start = before[..at]
        .chars()
        .next_back()
        .is_none_or(char::is_whitespace);
    let query = &before[at + 1..];
    (word_start && !query.contains(char::is_whitespace)).then(|| (at, query.to_owned()))
}

/// Where the names `picked` stand in `text`, each time it holds them.
fn mentioned_ranges(text: &str, picked: &[(String, ChatId)]) -> Vec<Range<usize>> {
    let mut ranges: Vec<Range<usize>> = picked
        .iter()
        .flat_map(|(name, _)| {
            text.match_indices(name.as_str())
                .map(|(at, name)| at..at + name.len())
        })
        .collect();
    ranges.sort_by_key(|range| range.start);
    ranges.dedup();
    ranges
}

/// `text` with each name `picked` written as WhatsApp reads a mention, `@`
/// and the digits of whom it names, and those it still names.
fn written_mentions(text: &str, mut picked: Vec<(String, ChatId)>) -> (String, Vec<ChatId>) {
    let mut text = text.to_owned();
    let mut mentioned = Vec::new();
    // Longest first, so "@Ana Maria" is not read as "@Ana".
    picked.sort_by_key(|(name, _)| std::cmp::Reverse(name.len()));
    for (name, address) in picked {
        let digits = address.as_str().split('@').next().unwrap_or_default();
        if text.contains(name.as_str()) && !digits.is_empty() {
            text = text.replace(name.as_str(), &format!("@{digits}"));
            mentioned.push(address);
        }
    }
    (text, mentioned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_at_starting_a_word_opens_the_list_until_a_space() {
        assert_eq!(typing_mention("hi @an", 6), Some((3, "an".to_owned())));
        assert_eq!(typing_mention("@", 1), Some((0, String::new())));
        assert_eq!(typing_mention("hi @ana there", 13), None);
        assert_eq!(typing_mention("mail a@b", 8), None);
    }

    #[test]
    fn colors_every_name_picked_where_it_stands() {
        let ana = ChatId::new("1@lid");
        let picked = [("@Ana".to_owned(), ana)];
        assert_eq!(mentioned_ranges("hi @Ana, @Ana!", &picked), [3..7, 9..13]);
        assert_eq!(mentioned_ranges("hi @An", &picked).len(), 0);
    }

    #[test]
    fn writes_the_names_picked_as_the_digits_whatsapp_reads() {
        let ana = ChatId::new("123456789@lid");
        let ana_maria = ChatId::new("5511912345678@s.whatsapp.net");
        let gone = ChatId::new("999999@lid");
        let (text, mentioned) = written_mentions(
            "@Ana and @Ana Maria, see",
            vec![
                ("@Ana".to_owned(), ana.clone()),
                ("@Ana Maria".to_owned(), ana_maria.clone()),
                ("@Bruno".to_owned(), gone),
            ],
        );
        assert_eq!(text, "@123456789 and @5511912345678, see");
        assert_eq!(mentioned, [ana_maria, ana]);
    }
}
