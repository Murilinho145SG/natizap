//! Every vote on a poll, option by option: who chose it, with their
//! picture and name.

use gpui::prelude::*;
use gpui::{Context, Div, FontWeight, SharedString, Window, div, px, rgb};
use natizap_protocol::Extra;
use natizap_ui::theme::{MUTED, PANEL, TEXT, glass};
use natizap_ui::{Button, ButtonVariant, Icon, dialog, separator};

use super::ConversationView;
use crate::items::Body;
use crate::poll::Voter;
use crate::poll_view::{Faces, face};

impl ConversationView {
    /// Shows every vote on the poll in bubble `poll`.
    pub(crate) fn show_votes(&mut self, poll: SharedString, cx: &mut Context<Self>) {
        self.votes_shown = Some(poll);
        cx.notify();
    }

    /// The window of a poll's votes, while one is shown.
    pub(super) fn votes_dialog(
        &mut self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        let id = self.votes_shown.clone()?;
        let bubble = self.bubble(&id, cx)?;
        let (
            Some(state),
            Body::Content {
                extra: Some(Extra::Poll(poll)),
                ..
            },
        ) = (&bubble.poll, &bubble.body)
        else {
            return None;
        };
        let faces = self.faces_of(state.chosen_by.iter().flatten(), cx);
        let options = poll
            .options
            .iter()
            .zip(&state.chosen_by)
            .map(|(option, voters)| option_votes(option, voters, &faces));
        let panel = div()
            .w(px(400.0))
            .max_h(px(560.0))
            .p_5()
            .flex()
            .flex_col()
            .gap_4()
            .rounded_xl()
            .bg(rgb(PANEL))
            .map(glass)
            .text_color(rgb(TEXT))
            .occlude()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(poll.question.clone()),
                    )
                    .child(
                        Button::new("close-votes")
                            .icon(Icon::Close)
                            .variant(ButtonVariant::Ghost)
                            .round()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.votes_shown = None;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .id("poll-votes")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .children(options),
            );
        let view = cx.weak_entity();
        Some(dialog(window, panel, move |_, cx| {
            let _ = view.update(cx, |this, cx| {
                this.votes_shown = None;
                cx.notify();
            });
        }))
    }

    /// The pictures of `voters`.
    fn faces_of<'a>(
        &mut self,
        voters: impl Iterator<Item = &'a Voter>,
        cx: &mut Context<Self>,
    ) -> Faces {
        voters
            .filter_map(|voter| voter.id.clone())
            .filter_map(|id| {
                let picture = self
                    .pictures
                    .update(cx, |pictures, cx| pictures.picture(&id, cx))?;
                Some((id, picture))
            })
            .collect()
    }
}

/// One option's votes: its name and count, then who chose it.
fn option_votes(option: &str, voters: &[Voter], faces: &Faces) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .flex()
                .items_center()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .font_weight(FontWeight::MEDIUM)
                        .child(option.to_owned()),
                )
                .child(div().text_size(px(12.5)).text_color(rgb(MUTED)).child(
                    match voters.len() {
                        1 => "1 vote".to_owned(),
                        count => format!("{count} votes"),
                    },
                )),
        )
        .children(voters.iter().map(|voter| {
            div()
                .flex()
                .items_center()
                .gap_3()
                .child(face(voter, faces))
                .child(div().truncate().child(voter.name.clone()))
        }))
        .child(div().pt_1().child(separator()))
}
