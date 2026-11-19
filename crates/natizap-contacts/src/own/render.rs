//! How your profile looks: the picture with its menu, and each line with
//! the field it is written in.

use gpui::prelude::*;
use gpui::{
    AnyElement, Context, Div, FontWeight, MouseButton, MouseDownEvent, ObjectFit, SharedString,
    Window, anchored, deferred, div, img, px, rgb, rgba, svg,
};
use natizap_protocol::Me;
use natizap_ui::theme::{ActiveTheme as _, DISPLAY_FONT, Theme};
use natizap_ui::{Button, ButtonVariant, Icon, Menu, avatar, separator};

use super::field::Field;
use super::{COVER, COVERED_PICTURE, Loaded, OwnProfile, OwnProfileClosed, PICTURE};
use crate::business_view::business_info;

impl Render for OwnProfile {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        let picture_error = self
            .error
            .as_ref()
            .filter(|(field, _)| field.is_none())
            .map(|(_, why)| error_line(why.clone(), theme));
        div()
            .id("own-profile")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(theme.panel))
            .text_color(rgb(theme.text))
            .child(title_bar(cx))
            .child(
                div()
                    .id("own-profile-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .items_center()
                    .children(self.cover.clone().map(|cover| {
                        img(cover)
                            .w_full()
                            .h(px(COVER))
                            .flex_none()
                            .object_fit(ObjectFit::Cover)
                    }))
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap_2()
                            .px_6()
                            .pb_8()
                            .when(self.cover.is_none(), Styled::pt_8)
                            .child(self.picture_view(cx))
                            .children(picture_error)
                            .child(div().w_full().py_4().child(separator(theme)))
                            .child(self.line(Field::Name, cx))
                            .child(self.line(Field::About, cx))
                            .child(phone_line(&self.me, theme))
                            .children(
                                match &self.business {
                                    Loaded::Ready(business) => business.as_ref(),
                                    Loaded::Loading | Loaded::Failed => None,
                                }
                                .map(|business| {
                                    div()
                                        .w_full()
                                        .flex()
                                        .flex_col()
                                        .gap_3()
                                        .child(div().w_full().py_4().child(separator(theme)))
                                        .child(
                                            div()
                                                .font_family(DISPLAY_FONT)
                                                .text_size(px(16.0))
                                                .font_weight(FontWeight::BOLD)
                                                .child("Business info"),
                                        )
                                        .child(business_info(business, theme))
                                        .child(
                                            div()
                                                .text_size(px(12.5))
                                                .text_color(rgb(theme.muted))
                                                .child(
                                                    "Change these from WhatsApp Business \
                                                 on your phone.",
                                                ),
                                        )
                                }),
                            ),
                    ),
            )
            .children(self.picture_menu(cx))
    }
}

impl OwnProfile {
    /// The picture, or your initials; a click on it offers to change it.
    fn picture_view(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = *cx.theme();
        let initials = self.me.name.clone().unwrap_or_default();
        let size = if self.cover.is_some() {
            COVERED_PICTURE
        } else {
            PICTURE
        };
        let picture = match &self.picture {
            Some(picture) => div().size(px(size)).child(
                img(picture.clone())
                    .size_full()
                    .rounded_full()
                    .object_fit(ObjectFit::Cover),
            ),
            None => avatar(self.me.phone.as_str(), &initials, None, px(size)),
        };
        div()
            .id("own-picture")
            .relative()
            .rounded_full()
            // Over a banner, it overlaps its foot, ringed by the panel.
            .when(self.cover.is_some(), |ring| {
                ring.mt(px(-size / 2.0))
                    .border_4()
                    .border_color(rgb(theme.panel))
            })
            .cursor_pointer()
            .group("own-picture")
            .when(self.uploading, |ring| ring.opacity(0.5))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|profile, event: &MouseDownEvent, _, cx| {
                    profile.picture_menu = Some(event.position);
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .child(picture)
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_1()
                    .bg(rgba(theme.scrim))
                    .text_size(px(12.5))
                    .invisible()
                    .group_hover("own-picture", Styled::visible)
                    .child(
                        svg()
                            .path(Icon::Camera.path())
                            .size(px(24.0))
                            .text_color(rgb(theme.text)),
                    )
                    .child(if self.uploading {
                        "Updating…"
                    } else {
                        "Change photo"
                    }),
            )
    }

    fn picture_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let position = self.picture_menu?;
        let view = cx.weak_entity();
        let uploading = view.clone();
        let mut menu = Menu::new().action(Icon::Camera, "Upload photo", move |_, cx| {
            let _ = uploading.update(cx, OwnProfile::upload_picture);
        });
        if self.picture.is_some() {
            let removing = view.clone();
            menu = menu.danger(Icon::Trash, "Remove photo", move |_, cx| {
                let _ = removing.update(cx, OwnProfile::remove_picture);
            });
        }
        let menu = menu.on_dismiss(move |_, cx| {
            let _ = view.update(cx, |profile, cx| {
                profile.picture_menu = None;
                cx.notify();
            });
        });
        Some(
            deferred(
                anchored()
                    .position(position)
                    .snap_to_window_with_margin(px(8.0))
                    .child(menu),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    /// A line of the profile: its label and value with a pencil to change
    /// it, or the field it is written in.
    fn line(&self, field: Field, cx: &mut Context<Self>) -> Div {
        let theme = *cx.theme();
        let row = div().w_full().flex().flex_col().gap_1().py_2().child(
            div()
                .text_size(px(12.5))
                .text_color(rgb(theme.muted))
                .child(field.label()),
        );
        let error = self
            .error
            .as_ref()
            .filter(|(failed, _)| *failed == Some(field))
            .map(|(_, why)| error_line(why.clone(), theme));
        if let Some(editing) = self
            .editing
            .as_ref()
            .filter(|editing| editing.field == field)
        {
            return row
                .child(
                    div()
                        .flex()
                        .items_end()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .pb_1()
                                .border_b_2()
                                .border_color(rgb(theme.text))
                                .text_size(px(15.0))
                                .child(editing.input.clone()),
                        )
                        .child(
                            Button::new(("save-own", field as usize))
                                .icon(Icon::Check)
                                .variant(ButtonVariant::Ghost)
                                .round()
                                .tooltip("Save (Enter)")
                                .on_click(cx.listener(move |profile, _, _, cx| {
                                    let Some(editing) = &profile.editing else {
                                        return;
                                    };
                                    let text = editing.input.read(cx).text().to_owned();
                                    profile.save(field, &text, cx);
                                })),
                        ),
                )
                .children(error);
        }
        let saving = self
            .saving
            .as_ref()
            .filter(|(saving, _)| *saving == field)
            .map(|(_, text)| text.clone());
        let (text, dim) = match (&saving, self.value(field)) {
            (Some(text), _) => (text.clone(), true),
            (None, Some(text)) => (text, false),
            (None, None) if field == Field::About && self.about == Loaded::Loading => {
                ("Loading…".to_owned(), true)
            }
            (None, None) if field == Field::About && self.about == Loaded::Failed => {
                ("WhatsApp is busy; it loads next time".to_owned(), true)
            }
            (None, None) => (String::new(), true),
        };
        row.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(15.0))
                        .when(field == Field::Name, |text| {
                            text.font_family(DISPLAY_FONT)
                                .font_weight(FontWeight::MEDIUM)
                        })
                        .when(dim, |text| text.text_color(rgb(theme.muted)))
                        .child(text),
                )
                .child(
                    Button::new(("edit-own", field as usize))
                        .icon(Icon::Edit)
                        .variant(ButtonVariant::Ghost)
                        .round()
                        .tooltip(format!("Change {}", field.label().to_lowercase()))
                        .on_click(cx.listener(move |profile, _, window, cx| {
                            profile.start_editing(field, window, cx);
                        })),
                ),
        )
        .children(error)
    }
}

fn title_bar(cx: &mut Context<OwnProfile>) -> Div {
    let theme = *cx.theme();
    div()
        .h(px(60.0))
        .flex_none()
        .flex()
        .items_center()
        .gap_3()
        .px_4()
        .border_b_1()
        .border_color(rgb(theme.line))
        .child(
            Button::new("close-own-profile")
                .icon(Icon::Back)
                .variant(ButtonVariant::Ghost)
                .round()
                .tooltip("Back to chats")
                .on_click(cx.listener(|_, _, _, cx| cx.emit(OwnProfileClosed))),
        )
        .child(
            div()
                .font_family(DISPLAY_FONT)
                .text_size(px(20.0))
                .font_weight(FontWeight::BOLD)
                .child("Profile"),
        )
}

fn phone_line(me: &Me, theme: Theme) -> Div {
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap_1()
        .py_2()
        .child(
            div()
                .text_size(px(12.5))
                .text_color(rgb(theme.muted))
                .child("Phone"),
        )
        .child(
            div()
                .text_size(px(15.0))
                .child(me.phone.phone_number().unwrap_or_default()),
        )
}

fn error_line(why: SharedString, theme: Theme) -> Div {
    div()
        .text_size(px(12.5))
        .text_color(rgb(theme.alert))
        .child(why)
}
