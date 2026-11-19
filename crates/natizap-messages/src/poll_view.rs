//! A poll inside its bubble: the question, each option with how many
//! chose it and the faces of who did, and a way to vote.

use std::collections::HashMap;
use std::path::PathBuf;

use gpui::prelude::*;
use gpui::{Div, ElementId, FontWeight, SharedString, WeakEntity, div, px, rgb, svg};
use natizap_protocol::{ChatId, Poll};
use natizap_ui::theme::{ACCENT, DEEP, LINE, MUTED, PANEL, TEXT};
use natizap_ui::{Icon, Tooltip, avatar, progress};

use crate::poll::{PollState, Voter};
use crate::view::ConversationView;

/// The pictures of people shown on polls, by address.
pub(crate) type Faces = HashMap<ChatId, PathBuf>;

/// How many faces show under an option before the rest are counted.
const MOST_FACES: usize = 5;
const FACE: f32 = 22.0;

/// A poll's question and choices, how many chose each, and a way to vote.
pub(crate) fn poll_view(
    id: &str,
    poll: &Poll,
    state: Option<&PollState>,
    faces: &Faces,
    view: &WeakEntity<ConversationView>,
) -> Div {
    let hint = if poll.multiple {
        "Select one or more"
    } else {
        "Select one"
    };
    let voters = state.map_or(0, |state| state.voters);
    let most = state.and_then(|state| state.counts().max()).unwrap_or(0);
    div()
        .min_w(px(260.0))
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .child(poll.question.clone()),
        )
        .child(div().text_size(px(12.5)).text_color(rgb(MUTED)).child(hint))
        .children(poll.options.iter().enumerate().map(|(index, option)| {
            let chosen_by = state
                .and_then(|state| state.chosen_by.get(index))
                .map_or(&[][..], Vec::as_slice);
            let count = chosen_by.len();
            let chosen = state.is_some_and(|state| state.mine.contains(option));
            #[expect(
                clippy::cast_precision_loss,
                reason = "a poll's voters, far below 2^23"
            )]
            let fraction = if voters == 0 {
                0.0
            } else {
                count as f32 / voters as f32
            };
            let row = div()
                .id(ElementId::NamedInteger(
                    format!("{id}-option").into(),
                    index as u64,
                ))
                .flex()
                .flex_col()
                .gap_1p5()
                .py_1()
                .child(option_line(
                    poll.multiple,
                    chosen,
                    option,
                    count,
                    count == most,
                ))
                .child(div().pl(px(28.0)).child(progress(fraction)))
                .when(!chosen_by.is_empty(), |row| {
                    row.child(
                        div()
                            .pl(px(28.0))
                            .child(stack(id, index, chosen_by, faces, view)),
                    )
                });
            let ballot = state.and_then(|state| state.ballot.clone());
            match (ballot, state) {
                (Some(ballot), Some(state)) => {
                    let options = state.after_click(option, poll.multiple, &poll.options);
                    let view = view.clone();
                    row.cursor_pointer()
                        .on_click(move |_, _, cx| {
                            let (ballot, options) = (ballot.clone(), options.clone());
                            let _ = view.update(cx, |view, cx| view.vote(&ballot, options, cx));
                        })
                        .into_any_element()
                }
                _ => row.into_any_element(),
            }
        }))
        .child(
            div()
                .pt_1()
                .text_size(px(12.5))
                .text_color(rgb(MUTED))
                .child(match voters {
                    1 => "1 vote".to_owned(),
                    voters => format!("{voters} votes"),
                }),
        )
        .when(state.is_some_and(|state| state.ballot.is_none()), |poll| {
            poll.child(
                div()
                    .text_size(px(12.5))
                    .text_color(rgb(MUTED))
                    .child("Vote on this poll from your phone"),
            )
        })
}

/// An option's mark, name and how many chose it, the count bold on the
/// leading option.
fn option_line(multiple: bool, chosen: bool, option: &str, count: usize, leading: bool) -> Div {
    div()
        .flex()
        .items_center()
        .gap_2p5()
        .child(mark(multiple, chosen))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_color(rgb(TEXT))
                .child(option.to_owned()),
        )
        .child(
            div()
                .text_size(px(12.5))
                .text_color(rgb(MUTED))
                .when(leading && count > 0, |count| {
                    count.font_weight(FontWeight::SEMIBOLD)
                })
                .child(count.to_string()),
        )
}

/// The faces of who chose an option, overlapping, each naming its person
/// under the pointer; a click shows every vote on the poll.
fn stack(
    id: &str,
    option: usize,
    voters: &[Voter],
    faces: &Faces,
    view: &WeakEntity<ConversationView>,
) -> impl IntoElement {
    let poll = SharedString::from(id.to_owned());
    let more = voters.len().saturating_sub(MOST_FACES);
    div()
        .id(ElementId::NamedInteger(
            format!("{id}-voters").into(),
            option as u64,
        ))
        .flex()
        .items_center()
        .cursor_pointer()
        .on_click({
            let view = view.clone();
            move |_, _, cx| {
                // Not a vote: the option row below takes clicks as votes.
                cx.stop_propagation();
                let poll = poll.clone();
                let _ = view.update(cx, |view, cx| view.show_votes(poll, cx));
            }
        })
        .children(
            voters
                .iter()
                .take(MOST_FACES)
                .enumerate()
                .map(|(place, voter)| {
                    face(voter, faces)
                        .id(ElementId::NamedInteger(
                            format!("{id}-{option}-voter").into(),
                            place as u64,
                        ))
                        .when(place > 0, |face| face.ml(px(-FACE / 3.0)))
                        .tooltip(Tooltip::text(voter.name.clone()))
                }),
        )
        .when(more > 0, |stack| {
            stack.child(
                div()
                    .pl_1p5()
                    .text_size(px(12.0))
                    .text_color(rgb(MUTED))
                    .child(format!("+{more}")),
            )
        })
}

/// A voter's round picture, ringed so overlapping faces stay apart.
pub(crate) fn face(voter: &Voter, faces: &Faces) -> Div {
    let seed = voter.id.as_ref().map_or("me", ChatId::as_str);
    let picture = voter.id.as_ref().and_then(|id| faces.get(id).cloned());
    div()
        .rounded_full()
        .border_2()
        .border_color(rgb(PANEL))
        .child(avatar(seed, &voter.name, picture, px(FACE)))
}

/// The box or circle beside an option, filled when this account chose it.
fn mark(multiple: bool, chosen: bool) -> Div {
    div()
        .size(px(18.0))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .when(multiple, |mark| mark.rounded(px(4.0)))
        .when(!multiple, Styled::rounded_full)
        .when(!chosen, |mark| mark.border_2().border_color(rgb(LINE)))
        .when(chosen, |mark| {
            mark.bg(rgb(ACCENT)).child(
                svg()
                    .path(Icon::Check.path())
                    .size(px(12.0))
                    .text_color(rgb(DEEP)),
            )
        })
}
