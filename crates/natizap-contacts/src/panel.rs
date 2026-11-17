use std::sync::Arc;

use gpui::prelude::*;
use gpui::{
    Context, Div, Entity, EventEmitter, FontWeight, Image, ImageFormat, ObjectFit, SharedString,
    Window, div, img, px, rgb,
};
use natizap_protocol::{ChatId, Profile, Protocol};
use natizap_ui::theme::{LINE, MUTED, PANEL, TEXT};
use natizap_ui::{Button, ButtonVariant, Icon, avatar, separator, skeleton, verified_seal};

/// Size of the picture at the top of the panel.
const PICTURE: f32 = 200.0;

/// The user closed the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProfileClosed;

/// A chat's profile, beside the conversation.
#[derive(Debug)]
pub struct ProfilePanel {
    chat: ChatId,
    title: SharedString,
    verified: bool,
    picture: Option<Arc<Image>>,
    profile: Option<Profile>,
}

impl EventEmitter<ProfileClosed> for ProfilePanel {}

impl ProfilePanel {
    /// Opens `chat`'s profile, named `title`, with a seal when `verified`,
    /// and fetches its details.
    pub fn new(
        protocol: &Entity<Protocol>,
        chat: ChatId,
        title: SharedString,
        verified: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        let picture = protocol.read(cx).profile_picture(&chat, true, cx);
        cx.spawn(async move |this, cx| match picture.await {
            Ok(Some(bytes)) => {
                let format = if bytes.starts_with(b"\x89PNG") {
                    ImageFormat::Png
                } else {
                    ImageFormat::Jpeg
                };
                let _ = this.update(cx, |panel, cx| {
                    panel.picture = Some(Arc::new(Image::from_bytes(format, bytes)));
                    cx.notify();
                });
            }
            Ok(None) => {}
            Err(error) => tracing::debug!(%error, "no full-size picture"),
        })
        .detach();
        let profile = protocol.read(cx).profile(&chat, cx);
        cx.spawn(async move |this, cx| {
            let profile = profile.await.unwrap_or_else(|error| {
                tracing::debug!(%error, "profile unavailable");
                Profile::default()
            });
            let _ = this.update(cx, |panel, cx| {
                panel.profile = Some(profile);
                cx.notify();
            });
        })
        .detach();
        Self {
            chat,
            title,
            verified,
            picture: None,
            profile: None,
        }
    }
}

impl Render for ProfilePanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let business = self
            .profile
            .as_ref()
            .and_then(|profile| profile.business.as_ref());
        let heading = if business.is_some() {
            "Business info"
        } else {
            "Contact info"
        };
        // A business shows the name WhatsApp verified, as the phone does.
        let name: SharedString = business
            .and_then(|business| business.name.clone())
            .map_or_else(|| self.title.clone(), Into::into);
        let picture = match &self.picture {
            Some(picture) => div().size(px(PICTURE)).child(
                img(picture.clone())
                    .size_full()
                    .rounded_full()
                    .object_fit(ObjectFit::Cover),
            ),
            None => avatar(self.chat.as_str(), &self.title, None, px(PICTURE)),
        };

        div()
            .id("profile-panel")
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(PANEL))
            .border_l_1()
            .border_color(rgb(LINE))
            .text_color(rgb(TEXT))
            .child(
                div()
                    .h(px(60.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_4()
                    .border_b_1()
                    .border_color(rgb(LINE))
                    .child(
                        Button::new("close-profile")
                            .icon(Icon::Close)
                            .variant(ButtonVariant::Ghost)
                            .round()
                            .tooltip("Close")
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(ProfileClosed))),
                    )
                    .child(div().font_weight(FontWeight::MEDIUM).child(heading)),
            )
            .child(
                div()
                    .id("profile-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_2()
                    .px_6()
                    .py_8()
                    .child(picture)
                    .child(
                        div()
                            .pt_4()
                            .text_center()
                            .text_size(px(22.0))
                            // The interface font at medium weight: the display
                            // font and heavier weights draw symbols such as ♥
                            // in their own monochrome glyphs instead of the
                            // emoji font's.
                            .font_weight(FontWeight::MEDIUM)
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(name)
                            .when(self.verified, |name| name.child(verified_seal(20.0))),
                    )
                    .children(self.chat.phone_number().map(|phone| {
                        div()
                            .text_size(px(15.0))
                            .text_color(rgb(MUTED))
                            .child(phone)
                    }))
                    .child(div().w_full().py_4().child(separator()))
                    .child(self.details()),
            )
    }
}

impl ProfilePanel {
    /// The about line and a business's details, or placeholders while they
    /// load.
    fn details(&self) -> Div {
        let Some(profile) = &self.profile else {
            return div()
                .w_full()
                .flex()
                .flex_col()
                .gap_3()
                .child(skeleton(
                    "about-label",
                    div().w(px(60.0)).h(px(10.0)).rounded_md(),
                ))
                .child(skeleton(
                    "about-text",
                    div().w_full().h(px(14.0)).rounded_md(),
                ));
        };
        let business = profile.business.as_ref();
        let fields = [
            ("About", profile.about.clone()),
            (
                "Category",
                business.and_then(|business| business.category.clone()),
            ),
            (
                "Description",
                business.and_then(|business| business.description.clone()),
            ),
            (
                "Address",
                business.and_then(|business| business.address.clone()),
            ),
            (
                "Email",
                business.and_then(|business| business.email.clone()),
            ),
            (
                "Website",
                business
                    .map(|business| business.websites.join("\n"))
                    .filter(|websites| !websites.is_empty()),
            ),
        ];
        div().w_full().flex().flex_col().gap_4().children(
            fields
                .into_iter()
                .filter_map(|(label, value)| Some(field(label, value?))),
        )
    }
}

fn field(label: &'static str, value: String) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_size(px(12.5))
                .text_color(rgb(MUTED))
                .child(label),
        )
        .child(div().text_size(px(14.5)).child(value))
}
