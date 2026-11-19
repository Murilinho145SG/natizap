//! Picking chats to send something to, e.g. a forwarded message.

use gpui::Focusable as _;
use gpui::prelude::*;
use gpui::{
    Context, Entity, EventEmitter, FontWeight, SharedString, Window, div, px, rgb, svg,
    uniform_list,
};
use natizap_profiles::ProfilePictures;
use natizap_protocol::ChatId;
use natizap_ui::theme::{ActiveTheme as _, Theme};
use natizap_ui::{Button, ButtonSize, Icon, TextInput, avatar, dialog};

use crate::list::{ChatList, ChatRow};

/// How many chats one forward may go to, as on the phone.
const MOST_CHOSEN: usize = 5;
const ROW_HEIGHT: f32 = 56.0;

/// The user chose these chats and pressed send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatsPicked(pub Vec<ChatId>);

/// The user backed out without choosing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PickerDismissed;

/// A searchable list of chats to choose up to five of, over the dimmed
/// window. Emits [`ChatsPicked`] or [`PickerDismissed`].
#[derive(Debug)]
pub struct ChatPicker {
    title: SharedString,
    list: Entity<ChatList>,
    pictures: Entity<ProfilePictures>,
    search: Entity<TextInput>,
    chosen: Vec<ChatId>,
}

impl EventEmitter<ChatsPicked> for ChatPicker {}
impl EventEmitter<PickerDismissed> for ChatPicker {}

impl ChatPicker {
    /// A picker headed `title` over `list`'s chats.
    pub fn new(
        title: impl Into<SharedString>,
        list: Entity<ChatList>,
        pictures: Entity<ProfilePictures>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| TextInput::new("Search", cx));
        // Typing narrows the list.
        cx.observe(&search, |_, _, cx| cx.notify()).detach();
        cx.observe(&pictures, |_, _, cx| cx.notify()).detach();
        window.focus(&search.focus_handle(cx), cx);
        Self {
            title: title.into(),
            list,
            pictures,
            search,
            chosen: Vec::new(),
        }
    }

    /// The chats that answer what was typed.
    fn matching(&self, cx: &Context<Self>) -> Vec<ChatRow> {
        let query = self.search.read(cx).text();
        self.list
            .read(cx)
            .rows()
            .iter()
            .filter(|row| row.matches(query))
            .cloned()
            .collect()
    }

    fn toggle(&mut self, chat: &ChatId, cx: &mut Context<Self>) {
        if let Some(index) = self.chosen.iter().position(|chosen| chosen == chat) {
            self.chosen.remove(index);
        } else if self.chosen.len() < MOST_CHOSEN {
            self.chosen.push(chat.clone());
        }
        cx.notify();
    }
}

impl Render for ChatPicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        let rows = self.matching(cx);
        let count = rows.len();
        let list = uniform_list(
            "picker-chats",
            count,
            cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                rows[range]
                    .iter()
                    .map(|row| {
                        let picture = this
                            .pictures
                            .update(cx, |pictures, cx| pictures.picture(&row.id, cx));
                        let checked = this.chosen.contains(&row.id);
                        let id = row.id.clone();
                        div()
                            .id(SharedString::from(row.id.as_str().to_owned()))
                            .h(px(ROW_HEIGHT))
                            .px_3()
                            .flex()
                            .items_center()
                            .gap_3()
                            .rounded_lg()
                            .cursor_pointer()
                            .hover(|row| row.bg(rgb(theme.raised)))
                            .on_click(cx.listener(move |this, _, _, cx| this.toggle(&id, cx)))
                            .child(avatar(row.id.as_str(), &row.title, picture, px(40.0)))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_color(rgb(theme.text))
                                    .child(row.title.clone()),
                            )
                            .child(check(checked, theme))
                    })
                    .collect()
            }),
        )
        .flex_1()
        .min_h_0();

        let panel = div()
            .id("chat-picker")
            .w(px(420.0))
            .h(px(560.0))
            .flex()
            .flex_col()
            .rounded_xl()
            .bg(rgb(theme.panel))
            .map(|surface| theme.glass(surface))
            .occlude()
            .on_key_down(cx.listener(|_, event: &gpui::KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    cx.emit(PickerDismissed);
                }
            }))
            .child(
                div()
                    .px_5()
                    .pt_5()
                    .pb_3()
                    .text_size(px(16.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(theme.text))
                    .child(self.title.clone()),
            )
            .child(
                div()
                    .mx_4()
                    .mb_2()
                    .px_4()
                    .py_2()
                    .rounded_full()
                    .bg(rgb(theme.raised))
                    .text_color(rgb(theme.text))
                    .child(self.search.clone()),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .px_2()
                    .child(list),
            )
            .child(self.footer(cx));
        let picker = cx.weak_entity();
        dialog(
            window,
            panel,
            move |_, cx| {
                let _ = picker.update(cx, |_, cx| cx.emit(PickerDismissed));
            },
            theme,
        )
    }
}

impl ChatPicker {
    /// How many are chosen, and send once there is one.
    fn footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        let chosen = self.chosen.len();
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap_3()
            .px_5()
            .py_3()
            .border_t_1()
            .border_color(rgb(theme.line))
            .child(
                div()
                    .flex_1()
                    .text_size(px(13.0))
                    .text_color(rgb(theme.muted))
                    .child(if chosen == 0 {
                        format!("Choose up to {MOST_CHOSEN} chats")
                    } else {
                        format!("{chosen} of {MOST_CHOSEN} chosen")
                    }),
            )
            .when(chosen > 0, |footer| {
                footer.child(
                    Button::new("picker-send")
                        .icon(Icon::Send)
                        .size(ButtonSize::Large)
                        .round()
                        .tooltip("Send")
                        .on_click(cx.listener(|this, _, _, cx| {
                            cx.emit(ChatsPicked(std::mem::take(&mut this.chosen)));
                        })),
                )
            })
    }
}

/// A round check: filled in the accent when chosen.
fn check(checked: bool, theme: Theme) -> impl IntoElement {
    div()
        .size(px(22.0))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .border_2()
        .border_color(rgb(if checked { theme.accent } else { theme.muted }))
        .when(checked, |check| check.bg(rgb(theme.accent)))
        .when(checked, |check| {
            check.child(
                svg()
                    .path(Icon::Check.path())
                    .size(px(14.0))
                    .text_color(rgb(theme.accent_ink)),
            )
        })
}
