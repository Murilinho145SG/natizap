//! A poll inside its bubble: the question, then each option as one row
//! filled as far as its share of the votes, with the faces of who chose it
//! and how many did; a click on a row votes.

use std::collections::HashMap;
use std::path::PathBuf;

use gpui::prelude::*;
use gpui::{Div, ElementId, FontWeight, SharedString, WeakEntity, div, px, relative, rgb, svg};
use natizap_protocol::{ChatId, Poll};
use natizap_ui::theme::{ACCENT, ACCENT_INK, MUTED, PANEL, TEXT};
use natizap_ui::{Icon, Tooltip, avatar};

use crate::poll::{PollState, Voter};
use crate::view::ConversationView;

/// The pictures of people shown on polls, by address.
pub(crate) type Faces = HashMap<ChatId, PathBuf>;

/// How many faces show on an option; its count says the rest.
const MOST_FACES: usize = 3;
const FACE: f32 = 18.0;
const ROW_HEIGHT: f32 = 36.0;

/// A poll's question and choices, how many chose each, and a way to vote.
pub(crate) fn poll_view(
    id: &str,
    poll: &Poll,
    state: Option<&PollState>,
    faces: &Faces,
    view: &WeakEntity<ConversationView>,
) -> Div {
    let voters = state.map_or(0, |state| state.voters);
    let hint = match (
        state.is_some_and(|state| state.ballot.is_none()),
        poll.multiple,
    ) {
        (true, _) => "Vote from your phone",
        (false, true) => "Select one or more",
        (false, false) => "Select one",
    };
    div()
        .min_w(px(280.0))
        .flex()
        .flex_col()
        .gap_1p5()
        .child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .child(poll.question.clone()),
        )
        .child(
            div()
                .pb_1()
                .text_size(px(12.0))
                .text_color(rgb(MUTED))
                .child(hint),
        )
        .children(poll.options.iter().enumerate().map(|(index, option)| {
            let chosen_by = state
                .and_then(|state| state.chosen_by.get(index))
                .map_or(&[][..], Vec::as_slice);
            let chosen = state.is_some_and(|state| state.mine.contains(option));
            #[expect(
                clippy::cast_precision_loss,
                reason = "a poll's voters, far below 2^23"
            )]
            let share = if voters == 0 {
                0.0
            } else {
                chosen_by.len() as f32 / voters as f32
            };
            let row = option_row(
                id,
                index,
                option,
                chosen_by,
                (poll.multiple, chosen),
                share,
                faces,
            );
            let ballot = state.and_then(|state| state.ballot.clone());
            match (ballot, state) {
                (Some(ballot), Some(state)) => {
                    let options = state.after_click(option, poll.multiple, &poll.options);
                    let view = view.clone();
                    row.cursor_pointer()
                        .hover(|row| row.opacity(0.85))
                        .on_click(move |_, _, cx| {
                            let (ballot, options) = (ballot.clone(), options.clone());
                            let _ = view.update(cx, |view, cx| view.vote(&ballot, options, cx));
                        })
                        .into_any_element()
                }
                _ => row.into_any_element(),
            }
        }))
        .child(footer(id, voters, view))
}

/// One option: filled behind as far as its `share` of the votes, in the
/// accent when this account chose it, with its mark, name, the faces of
/// who chose it and how many did.
fn option_row(
    id: &str,
    index: usize,
    option: &str,
    chosen_by: &[Voter],
    (multiple, chosen): (bool, bool),
    share: f32,
    faces: &Faces,
) -> gpui::Stateful<Div> {
    let fill = if chosen {
        rgb(ACCENT).opacity(0.22)
    } else {
        rgb(TEXT).opacity(0.07)
    };
    div()
        .id(ElementId::NamedInteger(
            format!("{id}-option").into(),
            index as u64,
        ))
        .relative()
        .h(px(ROW_HEIGHT))
        .rounded_lg()
        .overflow_hidden()
        .bg(rgb(TEXT).opacity(0.03))
        .child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .h_full()
                .w(relative(share))
                .bg(fill),
        )
        .child(
            div()
                .relative()
                .size_full()
                .px_2p5()
                .flex()
                .items_center()
                .gap_2p5()
                .child(mark(multiple, chosen))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(rgb(TEXT))
                        .child(option.to_owned()),
                )
                .child(stack(id, index, chosen_by, faces))
                .child(
                    div()
                        .min_w(px(16.0))
                        .text_size(px(12.5))
                        .text_right()
                        .text_color(rgb(if chosen { TEXT } else { MUTED }))
                        .child(chosen_by.len().to_string()),
                ),
        )
}

/// How many voted, and a way to every vote.
fn footer(id: &str, voters: usize, view: &WeakEntity<ConversationView>) -> Div {
    let poll = SharedString::from(id.to_owned());
    let view = view.clone();
    div()
        .pt_1()
        .flex()
        .items_center()
        .text_size(px(12.0))
        .text_color(rgb(MUTED))
        .child(div().flex_1().child(match voters {
            1 => "1 vote".to_owned(),
            voters => format!("{voters} votes"),
        }))
        .when(voters > 0, |footer| {
            footer.child(
                div()
                    .id(SharedString::from(format!("{id}-votes")))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(ACCENT))
                    .cursor_pointer()
                    .hover(|link| link.opacity(0.8))
                    .on_click(move |_, _, cx| {
                        let poll = poll.clone();
                        let _ = view.update(cx, |view, cx| view.show_votes(poll, cx));
                    })
                    .child("View votes"),
            )
        })
}

/// The faces of the first few who chose an option, overlapping, each
/// naming its person under the pointer.
fn stack(id: &str, option: usize, voters: &[Voter], faces: &Faces) -> Div {
    div()
        .flex()
        .items_center()
        .children(
            voters
                .iter()
                .take(MOST_FACES)
                .enumerate()
                .map(|(place, voter)| {
                    face_sized(voter, faces, FACE)
                        .id(ElementId::NamedInteger(
                            format!("{id}-{option}-voter").into(),
                            place as u64,
                        ))
                        .when(place > 0, |face| face.ml(px(-FACE / 3.0)))
                        .tooltip(Tooltip::text(voter.name.clone()))
                }),
        )
}

/// A voter's round picture, ringed so overlapping faces stay apart.
pub(crate) fn face(voter: &Voter, faces: &Faces) -> Div {
    face_sized(voter, faces, 32.0)
}

fn face_sized(voter: &Voter, faces: &Faces, size: f32) -> Div {
    let seed = voter.id.as_ref().map_or("me", ChatId::as_str);
    let picture = voter.id.as_ref().and_then(|id| faces.get(id).cloned());
    div()
        .rounded_full()
        .border_2()
        .border_color(rgb(PANEL))
        .child(avatar(seed, &voter.name, picture, px(size)))
}

/// The circle, or box when several may be chosen, beside an option: a
/// faint ring, filled with a check once this account chose it.
fn mark(multiple: bool, chosen: bool) -> Div {
    div()
        .size(px(16.0))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .when(multiple, |mark| mark.rounded(px(4.0)))
        .when(!multiple, Styled::rounded_full)
        .when(!chosen, |mark| mark.border_1().border_color(rgb(MUTED)))
        .when(chosen, |mark| {
            mark.bg(rgb(ACCENT)).child(
                svg()
                    .path(Icon::Check.path())
                    .size(px(11.0))
                    .text_color(rgb(ACCENT_INK)),
            )
        })
}
