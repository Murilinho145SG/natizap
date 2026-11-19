//! The main window's layout: the rail, the side column, the conversation
//! and a profile beside or over it, and what floats above them.

use gpui::prelude::*;
use gpui::{AnyElement, Context, Pixels, SharedString, div, px, rgb};
use natizap_session::SessionState;
use natizap_ui::motion::slide_in;
use natizap_ui::theme::LINE;
use natizap_ui::{Alert, AlertTone, RAIL_WIDTH};

use super::{Place, Root};

/// Width of the chat list, and of a profile beside the conversation.
const SIDE_PANEL: f32 = 380.0;
/// The narrowest a conversation gets with a profile beside it; narrower
/// windows show the profile over it.
const LEAST_CONVERSATION: f32 = 420.0;

impl Root {
    /// The window's content, laid out for a window `width` wide.
    pub(super) fn content(&mut self, width: Pixels, cx: &mut Context<Self>) -> AnyElement {
        if !self.shows_chats(cx) {
            return self.pairing.clone().into_any_element();
        }
        let reconnecting = matches!(self.session.read(cx).state(), SessionState::Reconnecting);
        div()
            .size_full()
            .relative()
            .flex()
            .child(self.rail(cx))
            .child(
                div()
                    .w(px(SIDE_PANEL))
                    .h_full()
                    .flex_none()
                    .border_r_1()
                    .border_color(rgb(LINE))
                    .map(
                        |pane| match self.own_profile.clone().filter(|_| self.own_shown) {
                            Some(profile) => pane.child(slide_in(
                                SharedString::from(format!("own-profile-{}", profile.entity_id())),
                                div().size_full().child(profile),
                                -24.0,
                            )),
                            None => match (self.place, &self.status_pane, &self.calls_pane) {
                                (Place::Status, Some(status), _) => pane.child(status.clone()),
                                (Place::Calls, _, Some(calls)) => pane.child(calls.clone()),
                                _ => pane.child(self.chat_list.clone()),
                            },
                        },
                    ),
            )
            .child(
                // Free to shrink below what it holds, so a long line in it
                // is cut or wrapped instead of pushing it past the window.
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(self.conversation.clone()),
            )
            .children(self.profile.clone().map(|profile| {
                // Each profile slides in from the side it opens on: beside the
                // conversation when the window has room for both, over it
                // otherwise, so the conversation never shrinks to nothing.
                let id = SharedString::from(format!("profile-{}", profile.entity_id()));
                let beside = f32::from(width) - RAIL_WIDTH - 2.0 * SIDE_PANEL >= LEAST_CONVERSATION;
                let panel = slide_in(id, div().size_full().child(profile), 24.0);
                if beside {
                    div().w(px(SIDE_PANEL)).h_full().flex_none().child(panel)
                } else {
                    div()
                        .absolute()
                        .top_0()
                        .right_0()
                        .w(px(SIDE_PANEL))
                        .h_full()
                        .border_l_1()
                        .border_color(rgb(LINE))
                        .shadow_lg()
                        // Over the conversation: its clicks stay here, or
                        // the header below would open the profile again.
                        .occlude()
                        .child(panel)
                }
            }))
            .children(self.picker.clone())
            .children(self.settings.clone())
            .children(self.status_viewer.clone())
            .children(self.status_composer.clone())
            // Chats stay readable offline, so the news floats over them
            // instead of replacing them.
            .when(reconnecting, |content| {
                content.child(
                    div()
                        .absolute()
                        .top_3()
                        .left_0()
                        .right_0()
                        .flex()
                        .justify_center()
                        .child(
                            Alert::new(AlertTone::Warning, "Reconnecting…")
                                .description("The connection dropped. natizap keeps trying."),
                        ),
                )
            })
            .into_any_element()
    }
}
