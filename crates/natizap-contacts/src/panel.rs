use std::sync::Arc;

use gpui::prelude::*;
use gpui::{
    Context, Div, Entity, EventEmitter, FontWeight, Image, ImageFormat, ObjectFit, Pixels, Point,
    SharedString, Stateful, Window, div, img, px, rgb,
};
use std::collections::HashMap;

use natizap_profiles::ProfilePictures;
use natizap_protocol::{Business, ChatId, GroupInfo, PersonName, Profile, Protocol};

use crate::hours;
use crate::sections::{ChatSettings, ChatSummary, DisappearingChosen, MuteToggled};
use natizap_ui::theme::{ALERT, LINE, LINK, MINT, MUTED, PANEL, TEXT};
use natizap_ui::{Button, ButtonVariant, Icon, avatar, dialog, separator, skeleton, verified_seal};

/// Size of the picture at the top of the panel.
const PICTURE: f32 = 200.0;
/// Size of the picture when it overlaps a banner, as on the phone.
const COVERED_PICTURE: f32 = 140.0;
/// Height of a business's banner.
const COVER: f32 = 150.0;

/// The user wants to write to the person whose profile this is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatWanted {
    /// Their address.
    pub chat: ChatId,
    /// Their name, until the chat has one.
    pub title: SharedString,
}

/// The user closed the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProfileClosed;

/// A chat's profile, beside the conversation.
#[derive(Debug)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "verified, own, enlarged and all members listed are independent \
              facts about what the panel shows"
)]
pub struct ProfilePanel {
    chat: ChatId,
    title: SharedString,
    verified: bool,
    /// The chat is this account's own.
    own: bool,
    picture: Option<Arc<Image>>,
    /// A business's banner.
    cover: Option<Arc<Image>>,
    /// The picture is shown full size over the window.
    enlarged: bool,
    profile: Option<Profile>,
    /// A group's details, for a group's profile.
    group: Option<GroupInfo>,
    pub(crate) protocol: Entity<Protocol>,
    pub(crate) pictures: Entity<ProfilePictures>,
    /// What people are called, to name a group's members.
    pub(crate) names: Arc<HashMap<ChatId, PersonName>>,
    /// Every member is listed, not only the first.
    pub(crate) all_members: bool,
    pub(crate) summary: Option<ChatSummary>,
    pub(crate) settings: Option<ChatSettings>,
    /// Where the disappearing messages choices are open.
    pub(crate) timer_menu: Option<Point<Pixels>>,
}

impl EventEmitter<ProfileClosed> for ProfilePanel {}
impl EventEmitter<ChatWanted> for ProfilePanel {}
impl EventEmitter<crate::members::PersonOpened> for ProfilePanel {}

impl ProfilePanel {
    /// Opens `chat`'s profile, named `title`, with a seal when `verified`,
    /// and fetches its details.
    pub fn new(
        protocol: &Entity<Protocol>,
        pictures: &Entity<ProfilePictures>,
        chat: ChatId,
        title: SharedString,
        verified: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(pictures, |_, _, cx| cx.notify()).detach();
        let picture = protocol.read(cx).profile_picture(&chat, true, cx);
        cx.spawn(async move |this, cx| match picture.await {
            Ok(Some(bytes)) => {
                let _ = this.update(cx, |panel, cx| {
                    panel.picture = Some(image(bytes));
                    cx.notify();
                });
            }
            Ok(None) => {}
            Err(error) => tracing::debug!(%error, "no full-size picture"),
        })
        .detach();
        if chat.is_group() {
            let group = protocol.read(cx).group_info(&chat, cx);
            cx.spawn(async move |this, cx| match group.await {
                Ok(group) => {
                    let _ = this.update(cx, |panel, cx| {
                        panel.group = Some(group);
                        cx.notify();
                    });
                }
                Err(error) => tracing::debug!(%error, "group details unavailable"),
            })
            .detach();
        }
        let profile = protocol.read(cx).profile(&chat, cx);
        cx.spawn(async move |this, cx| {
            let profile = profile.await.unwrap_or_else(|error| {
                tracing::debug!(%error, "profile unavailable");
                Profile::default()
            });
            let _ = this.update(cx, |panel, cx| {
                panel.cover = profile
                    .business
                    .as_ref()
                    .and_then(|business| business.cover.clone())
                    .map(image);
                panel.profile = Some(profile);
                cx.notify();
            });
        })
        .detach();
        Self {
            own: protocol.read(cx).is_own(&chat),
            chat,
            title,
            verified,
            picture: None,
            cover: None,
            enlarged: false,
            profile: None,
            group: None,
            protocol: protocol.clone(),
            pictures: pictures.clone(),
            names: Arc::default(),
            all_members: false,
            summary: None,
            settings: None,
            timer_menu: None,
        }
    }

    /// Names a group's members by `names`, by address.
    pub fn set_names(&mut self, names: Arc<HashMap<ChatId, PersonName>>, cx: &mut Context<Self>) {
        self.names = names;
        cx.notify();
    }

    /// Shows the chat's media and starred messages.
    pub fn set_summary(&mut self, summary: ChatSummary, cx: &mut Context<Self>) {
        self.summary = Some(summary);
        cx.notify();
    }

    /// Shows how the chat is set up: notifications and disappearing
    /// messages.
    pub fn set_settings(&mut self, settings: ChatSettings, cx: &mut Context<Self>) {
        self.settings = Some(settings);
        cx.notify();
    }
}

impl EventEmitter<MuteToggled> for ProfilePanel {}
impl EventEmitter<DisappearingChosen> for ProfilePanel {}

impl Render for ProfilePanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("profile-panel")
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(PANEL))
            .border_l_1()
            .border_color(rgb(LINE))
            .text_color(rgb(TEXT))
            .child(self.title_bar(cx))
            .child(
                div()
                    .id("profile-body")
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
                            .child(self.picture(cx))
                            .child(self.identity())
                            .children(self.message_button(cx))
                            .child(div().w_full().py_4().child(separator()))
                            .child(self.details())
                            .child(self.sections(cx))
                            .children(self.group.clone().map(|group| self.members(&group, cx))),
                    ),
            )
            .when(self.enlarged, |panel| {
                panel.children(self.picture.clone().map(|picture| {
                    let side = window
                        .viewport_size()
                        .height
                        .min(window.viewport_size().width)
                        * 0.8;
                    let entity = cx.weak_entity();
                    dialog(
                        window,
                        img(picture)
                            .size(side)
                            .rounded(px(12.0))
                            .object_fit(ObjectFit::Cover),
                        move |_, cx| {
                            let _ = entity.update(cx, |panel, cx| {
                                panel.enlarged = false;
                                cx.notify();
                            });
                        },
                    )
                }))
            })
    }
}

impl ProfilePanel {
    fn title_bar(&self, cx: &mut Context<Self>) -> Div {
        let heading = if self.chat.is_group() {
            "Group info"
        } else if self.business().is_some() {
            "Business info"
        } else {
            "Contact info"
        };
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
            .child(div().font_weight(FontWeight::MEDIUM).child(heading))
    }

    fn business(&self) -> Option<&Business> {
        self.profile.as_ref()?.business.as_ref()
    }

    /// The picture, ringed and overlapping the banner when there is one;
    /// clicking it shows it full size.
    fn picture(&self, cx: &mut Context<Self>) -> Stateful<Div> {
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
            None => avatar(self.chat.as_str(), &self.title, None, px(size)),
        };
        div()
            .id("profile-picture")
            .rounded_full()
            .when(self.cover.is_some(), |ring| {
                ring.mt(px(-size / 2.0)).border_4().border_color(rgb(PANEL))
            })
            .when(self.picture.is_some(), |ring| {
                ring.cursor_pointer()
                    .on_click(cx.listener(|panel, _, _, cx| {
                        panel.enlarged = true;
                        cx.notify();
                    }))
            })
            .child(picture)
    }

    /// The name, number, category and opening hours under the picture.
    /// A way to write to the person, unless this is a group or yourself.
    fn message_button(&self, cx: &mut Context<Self>) -> Option<Button> {
        if self.chat.is_group() || self.own {
            return None;
        }
        let wanted = ChatWanted {
            chat: self.chat.clone(),
            title: self.title.clone(),
        };
        Some(
            Button::new("message-person")
                .icon(Icon::Chats)
                .label("Message")
                .variant(ButtonVariant::Secondary)
                .on_click(cx.listener(move |_, _, _, cx| cx.emit(wanted.clone()))),
        )
    }

    fn identity(&self) -> Div {
        let business = self.business();
        // A business shows the name WhatsApp verified, as the phone does.
        let mut name: String = business
            .and_then(|business| business.name.clone())
            .unwrap_or_else(|| self.title.to_string());
        if self.own {
            name.push_str(" (You)");
        }
        let verified = self.verified || business.is_some_and(|business| business.verified);
        let today = business.and_then(|business| hours::today(&business.hours));
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap_1()
            .pt_3()
            .child(
                div()
                    .text_center()
                    .text_size(px(22.0))
                    // The interface font at medium weight: the display font
                    // and heavier weights draw symbols such as ♥ in their
                    // own monochrome glyphs instead of the emoji font's.
                    .font_weight(FontWeight::MEDIUM)
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(name)
                    .when(verified, |name| name.child(verified_seal(20.0))),
            )
            .children(self.chat.phone_number().map(|phone| {
                div()
                    .text_size(px(15.0))
                    .text_color(rgb(MUTED))
                    .child(phone)
            }))
            .children(self.group.as_ref().map(|group| {
                let kind = if group.community.is_some() {
                    "Community group"
                } else {
                    "Group"
                };
                div()
                    .text_size(px(15.0))
                    .text_color(rgb(MUTED))
                    .child(format!("{kind} · {} members", group.members.len()))
            }))
            .children(
                business
                    .and_then(|business| business.category.clone())
                    .map(|category| {
                        div()
                            .text_size(px(13.5))
                            .text_color(rgb(MUTED))
                            .child(category)
                    }),
            )
            .children(today.map(|today| {
                let status = today.open.map(|open| {
                    let (label, color) = if open {
                        ("Open now", MINT)
                    } else {
                        ("Closed now", ALERT)
                    };
                    div().text_color(rgb(color)).child(label)
                });
                div()
                    .pt_1()
                    .flex()
                    .gap_1p5()
                    .text_size(px(13.5))
                    .children(status)
                    .child(div().text_color(rgb(MUTED)).child(today.hours))
            }))
    }
}

impl ProfilePanel {
    /// The about line and a business's details, or placeholders while they
    /// load.
    fn details(&self) -> Div {
        if let Some(group) = &self.group {
            return div().w_full().flex().flex_col().gap_4().children(
                group
                    .description
                    .clone()
                    .map(|description| field("Description", description)),
            );
        }
        let Some(profile) = &self.profile.as_ref().filter(|_| !self.chat.is_group()) else {
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
                "Description",
                business.and_then(|business| business.description.clone()),
            ),
            (
                "Address",
                business.and_then(|business| business.address.clone()),
            ),
        ];
        let email = business
            .and_then(|business| business.email.clone())
            .map(|email| links("Email", vec![(format!("mailto:{email}"), email)]));
        let websites = business
            .map(|business| {
                business
                    .websites
                    .iter()
                    .map(|site| {
                        let target = if site.contains("://") {
                            site.clone()
                        } else {
                            format!("https://{site}")
                        };
                        (target, site.clone())
                    })
                    .collect::<Vec<_>>()
            })
            .filter(|websites| !websites.is_empty())
            .map(|websites| links("Website", websites));
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap_4()
            .children(
                fields
                    .into_iter()
                    .filter_map(|(label, value)| Some(field(label, value?))),
            )
            .children(email)
            .children(websites)
    }
}

fn field(label: &'static str, value: String) -> Div {
    labeled(label).child(div().text_size(px(14.5)).child(value))
}

fn labeled(label: &'static str) -> Div {
    div().flex().flex_col().gap_1().child(
        div()
            .text_size(px(12.5))
            .text_color(rgb(MUTED))
            .child(label),
    )
}

/// A field of `(target, text)` links, each opened by the system.
fn links(label: &'static str, links: Vec<(String, String)>) -> Div {
    labeled(label).children(
        links
            .into_iter()
            .enumerate()
            .map(|(index, (target, text))| {
                div()
                    .id((label, index))
                    .text_size(px(14.5))
                    .text_color(rgb(LINK))
                    .cursor_pointer()
                    .hover(Styled::underline)
                    .on_click(move |_, _, cx| cx.open_url(&target))
                    .child(text)
            }),
    )
}

/// Decodes a picture WhatsApp sent as JPEG, PNG or WebP.
fn image(bytes: Vec<u8>) -> Arc<Image> {
    let format = if bytes.starts_with(b"\x89PNG") {
        ImageFormat::Png
    } else if bytes.get(8..12) == Some(b"WEBP") {
        ImageFormat::Webp
    } else {
        ImageFormat::Jpeg
    };
    Arc::new(Image::from_bytes(format, bytes))
}
