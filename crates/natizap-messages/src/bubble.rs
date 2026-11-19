//! How each row of a conversation looks.

use gpui::prelude::*;
use gpui::{
    Animation, AnimationExt as _, AnyElement, App, BoxShadow, Div, FontWeight, Hsla, MouseButton,
    SharedString, Stateful, WeakEntity, div, point, px, relative, rgb, rgba, svg,
};
use natizap_protocol::{ContentKind, Extra, Forwarding};
use natizap_ui::motion::ease_out;
use natizap_ui::theme::{
    ACCENT, ALERT, DEEP, LINE, MINT, MUTED, OUTGOING, PANEL, RAISED, SCRIM, TEXT, glass,
};
use natizap_ui::{Button, ButtonSize, ButtonVariant, Icon, avatar};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use crate::authors::Author;
use crate::extra_view::{render_buttons, render_extra};
use crate::items::{Body, Bubble, Item, MediaView, QuotePreview};
use crate::media_view::render_media;
use crate::message::Delivery;
use crate::quote::{name_and_phone, quoted};
use crate::rich_text::{TextFocus, rich_text, selectable};
use crate::view::{ConversationView, ProfileRequested};
use crate::voice::VoiceUi;

/// How attachments show in this draw: the voice note playing, and the
/// downloads under way and how far along each is, by message id.
pub(crate) struct Attachments {
    pub(crate) voice: VoiceUi,
    pub(crate) downloads: HashMap<String, f32>,
}

impl Attachments {
    fn progress(&self, media: &MediaView) -> Option<f32> {
        self.downloads.get(&media.fetch.message_id).copied()
    }
}
use crate::voice_view::render_voice;

/// Size of a group sender's picture beside their messages.
const SENDER_PICTURE: f32 = 28.0;

/// Quick reactions offered on every bubble, as in WhatsApp.
pub(crate) const QUICK_REACTIONS: [&str; 6] = ["👍", "❤️", "😂", "😮", "😢", "🙏"];

/// How long a message gone to stays lit up.
pub(crate) const LIT_FOR: Duration = Duration::from_millis(1600);

/// A bubble just gone to: its rim glows in the accent and fades back,
/// marking where the eye should land without moving anything around it.
fn lit_up(id: &str, shape: Div) -> AnyElement {
    shape
        .with_animation(
            SharedString::from(format!("lit-{id}")),
            Animation::new(LIT_FOR).with_easing(ease_out),
            |shape, delta| {
                let mut rim: Hsla = rgb(ACCENT).into();
                rim.a = 1.0 - delta;
                let mut glow = rim;
                glow.a = 0.5 * (1.0 - delta);
                shape.border_color(rim).shadow(vec![BoxShadow {
                    color: glow,
                    offset: point(px(0.0), px(0.0)),
                    blur_radius: px(16.0),
                    spread_radius: px(1.0),
                    inset: false,
                }])
            },
        )
        .into_any_element()
}

pub(crate) fn render_item(
    item: &Item,
    view: &WeakEntity<ConversationView>,
    reacting: Option<&SharedString>,
    shown: &Attachments,
    focus: &TextFocus,
    picture: Option<PathBuf>,
    lit: bool,
) -> AnyElement {
    match item {
        Item::Day(label) => div()
            .w_full()
            .flex()
            .justify_center()
            .pt_4()
            .pb_2()
            .child(
                div()
                    .px_3()
                    .py_1()
                    .rounded_full()
                    .bg(rgb(PANEL))
                    .text_size(px(12.0))
                    .text_color(rgb(MUTED))
                    .child(label.clone()),
            )
            .into_any_element(),
        Item::Bubble(bubble) => render_bubble(
            bubble,
            view,
            reacting.is_some_and(|id| *id == bubble.id),
            shown,
            focus,
            picture,
            lit,
        ),
    }
}

fn render_bubble(
    bubble: &Bubble,
    view: &WeakEntity<ConversationView>,
    reacting: bool,
    shown: &Attachments,
    focus: &TextFocus,
    picture: Option<PathBuf>,
    lit: bool,
) -> AnyElement {
    // In groups, others' messages make room on the left for their sender's
    // picture, shown beside the first of a burst, as on the phone.
    let gutter = bubble
        .sender
        .as_ref()
        .filter(|_| !bubble.from_me)
        .map(|sender| {
            div().w(px(SENDER_PICTURE)).flex_none().self_start().when(
                bubble.author.is_some(),
                |gutter| {
                    gutter.child(
                        div()
                            .id(SharedString::from(format!("sender-{}", bubble.id)))
                            .cursor_pointer()
                            .hover(|picture| picture.opacity(0.8))
                            .on_click(open_author(sender, view))
                            .child(avatar(
                                sender.id.as_str(),
                                sender.name.trim_start_matches("~ "),
                                picture,
                                px(SENDER_PICTURE),
                            )),
                    )
                },
            )
        });
    let message = div()
        .max_w(relative(0.72))
        .flex()
        .flex_col()
        .gap_0p5()
        .when(bubble.from_me, Styled::items_end)
        .when(!bubble.from_me, Styled::items_start)
        .child({
            let shape = shape(bubble, view, shown, focus).max_w_full();
            if lit {
                lit_up(&bubble.id, shape)
            } else {
                shape.into_any_element()
            }
        })
        .when(!bubble.reactions.is_empty(), |message| {
            message.child(reaction_chips(bubble))
        });
    let actions = if reacting {
        quick_reactions(bubble, view).into_any_element()
    } else {
        hover_actions(bubble, view).into_any_element()
    };
    let menu = {
        let (view, id) = (view.clone(), bubble.id.clone());
        move |event: &gpui::MouseDownEvent, _: &mut gpui::Window, cx: &mut App| {
            let _ = view.update(cx, |view, cx| {
                view.open_message_menu(id.clone(), event.position, cx);
            });
        }
    };
    div()
        .id(bubble.id.clone())
        .group("bubble")
        .on_mouse_down(MouseButton::Right, menu)
        .w_full()
        .flex()
        .items_center()
        .gap_1()
        .pt(px(if bubble.starts_burst { 8.0 } else { 2.0 }))
        .when(bubble.from_me, Styled::justify_end)
        .children(gutter)
        .map(|row| {
            // Actions sit on the open side of the bubble.
            if bubble.from_me {
                row.child(actions).child(message)
            } else {
                row.child(message).child(actions)
            }
        })
        .into_any_element()
}

/// The bubble itself: author, attachment, quote, content and time.
/// A business's buttons, which go under the text and time.
fn buttons_under(bubble: &Bubble, view: &WeakEntity<ConversationView>) -> Option<Div> {
    match &bubble.body {
        Body::Content {
            extra: Some(Extra::Buttons(buttons)),
            ..
        } => Some(render_buttons(&bubble.id, buttons, &bubble.reply, view)),
        _ => None,
    }
}

/// The bubble's time, with its edited mark, star and ticks.
fn meta(bubble: &Bubble) -> Div {
    div()
        .flex()
        .items_center()
        .justify_end()
        .gap_1p5()
        .text_size(px(11.0))
        .text_color(rgb(MUTED))
        .when(
            matches!(bubble.body, Body::Content { edited: true, .. }),
            |meta| meta.child("edited"),
        )
        .when(bubble.starred, |meta| {
            meta.child(
                svg()
                    .path(Icon::Star.path())
                    .size(px(12.0))
                    .text_color(rgb(MUTED)),
            )
        })
        .child(bubble.time.clone())
        .when(bubble.from_me, |meta| {
            meta.child(delivery_mark(bubble.delivery))
        })
}

fn shape(
    bubble: &Bubble,
    view: &WeakEntity<ConversationView>,
    shown: &Attachments,
    focus: &TextFocus,
) -> Div {
    let meta = meta(bubble);
    // A voice note carries no text, so its time and ticks sit inside the
    // player instead of on a line of their own, as on the phone.
    let voice_note = bubble.media.as_ref().filter(|media| {
        matches!(
            media.fetch.kind,
            ContentKind::VoiceMessage | ContentKind::Audio
        )
    });
    if let Some(media) = voice_note {
        let downloading = shown.progress(media);
        return padded(bubble, view).child(render_voice(
            media,
            &shown.voice,
            downloading,
            meta,
            view,
        ));
    }
    if let Some(sticker) = bubble
        .media
        .as_ref()
        .filter(|media| media.fetch.kind == ContentKind::Sticker)
    {
        return sticker_bubble(bubble, sticker, meta, view, shown.progress(sticker));
    }
    let picture = bubble
        .media
        .as_ref()
        .filter(|media| matches!(media.fetch.kind, ContentKind::Photo | ContentKind::Video));
    if let Some(media) = picture {
        return picture_bubble(bubble, media, meta, view, focus, shown.progress(media));
    }

    let body = match &bubble.body {
        Body::Deleted => div()
            .italic()
            .text_color(rgb(MUTED))
            .child("This message was deleted"),
        Body::MissedCall { video } => missed_call(*video),
        Body::Content {
            media, text, extra, ..
        } => div()
            .flex()
            .flex_col()
            .gap_0p5()
            .children(
                extra
                    .as_ref()
                    .and_then(|extra| render_extra(&bubble.id, extra, bubble.poll.as_ref(), view)),
            )
            .children(media.map(|media| {
                div()
                    .text_size(px(12.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(MUTED))
                    .child(media)
            }))
            .map(|body| {
                let (shown, hits) = match text {
                    Some(text) => {
                        let (shown, hits) = rich_text(&bubble.id, text, view, focus);
                        (Some(shown), hits)
                    }
                    None => (None, Vec::new()),
                };
                selectable(body.children(shown), hits, view)
            }),
    };

    padded(bubble, view)
        .children(
            bubble
                .media
                .as_ref()
                .map(|media| render_media(media, view, shown.progress(media))),
        )
        .children(
            bubble
                .quote
                .as_ref()
                .map(|quote| jump_to_quoted(quote, view)),
        )
        // Time sits beside short text and drops below text that fills the
        // bubble, as in WhatsApp.
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_end()
                .gap_x_2()
                // Without a max width the text keeps its unwrapped width and
                // overflows the bubble.
                .child(body.min_w_0().max_w_full())
                .child(meta.ml_auto().pb(px(1.0))),
        )
        .children(buttons_under(bubble, view))
}

/// A photo or video filling its bubble but for a thin rim, as on the
/// phone: the time sits on the picture, or beside the caption if any.
/// A sticker, as the phone shows it: no bubble, just the sticker, with
/// its author's name above it in groups and its time on a dark pill over
/// its corner.
fn sticker_bubble(
    bubble: &Bubble,
    media: &MediaView,
    meta: Div,
    view: &WeakEntity<ConversationView>,
    downloading: Option<f32>,
) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .when(bubble.from_me, Styled::items_end)
        .children(author(bubble, view).map(Styled::px_1))
        .children(forwarded(bubble).map(Styled::px_1))
        .children(
            bubble
                .quote
                .as_ref()
                .map(|quote| jump_to_quoted(quote, view)),
        )
        .child(
            div()
                .relative()
                .child(render_media(media, view, downloading))
                .child(
                    div()
                        .absolute()
                        .bottom(px(4.0))
                        .right(px(4.0))
                        .px_1p5()
                        .rounded_full()
                        .bg(rgba(SCRIM))
                        .child(meta.text_color(rgb(TEXT))),
                ),
        )
}

/// The body of a call nobody answered: its icon and what it was.
fn missed_call(video: bool) -> Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(
            div()
                .size(px(30.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .bg(rgb(DEEP))
                .child(
                    svg()
                        .path(Icon::CallMissed.path())
                        .size(px(15.0))
                        .text_color(rgb(ALERT)),
                ),
        )
        .child(div().font_weight(FontWeight::MEDIUM).child(if video {
            ContentKind::MissedVideoCall.label()
        } else {
            ContentKind::MissedVoiceCall.label()
        }))
}

/// The bubble's padding around a picture.
const PICTURE_PADDING: f32 = 3.0;

fn picture_bubble(
    bubble: &Bubble,
    media: &MediaView,
    meta: Div,
    view: &WeakEntity<ConversationView>,
    focus: &TextFocus,
    downloading: Option<f32>,
) -> Div {
    let caption = match &bubble.body {
        Body::Content {
            text: Some(text), ..
        } => Some(text),
        _ => None,
    };
    let picture = div()
        .relative()
        .child(render_media(media, view, downloading));
    let (picture, caption) = match caption {
        Some(text) => {
            let (shown, hits) = rich_text(&bubble.id, text, view, focus);
            // As wide as the bubble, which is as wide as the picture, so
            // long captions wrap under it instead of spilling past it.
            let caption = div()
                .w_full()
                .px(px(7.0))
                .pb(px(3.0))
                .flex()
                .flex_wrap()
                .items_end()
                .gap_x_2()
                // Text measures itself at its longest line; held to the
                // caption's width, it wraps there instead of running past
                // the bubble, and a short caption still leaves the time room
                // beside it.
                .child(div().max_w_full().min_w_0().child(shown))
                .child(meta.ml_auto().pb(px(1.0)));
            (picture, Some(selectable(caption, hits, view)))
        }
        None => (
            picture.child(
                div()
                    .absolute()
                    .bottom(px(6.0))
                    .right(px(6.0))
                    .px_1p5()
                    .rounded_full()
                    .bg(rgba(SCRIM))
                    .child(meta.text_color(rgb(TEXT))),
            ),
            None,
        ),
    };
    // A width of its own, the picture's and its padding, narrowing with
    // the window: a caption measured against anything wider wraps into
    // more lines than were laid out and spills out of the bubble.
    frame(bubble)
        .w(px(media.size.0 + 2.0 * PICTURE_PADDING))
        .max_w_full()
        .p(px(PICTURE_PADDING))
        .gap_1()
        .children(author(bubble, view).map(|name| name.px(px(7.0)).pt(px(2.0))))
        .children(forwarded(bubble).map(|line| line.px(px(7.0))))
        .children(
            bubble
                .quote
                .as_ref()
                .map(|quote| jump_to_quoted(quote, view)),
        )
        .child(picture)
        .children(caption)
}

/// [`frame`] with the room text needs around it.
fn padded(bubble: &Bubble, view: &WeakEntity<ConversationView>) -> Div {
    frame(bubble)
        .gap_1()
        .px(px(10.0))
        .py(px(6.0))
        .children(author(bubble, view))
        .children(forwarded(bubble))
}

/// "Forwarded", or "Forwarded many times", as the phone marks them.
fn forwarded(bubble: &Bubble) -> Option<Div> {
    let label = match bubble.forwarding? {
        Forwarding::Once => "Forwarded",
        Forwarding::Often => "Forwarded many times",
    };
    Some(
        div()
            .flex()
            .items_center()
            .gap_1()
            .text_size(px(12.5))
            .italic()
            .text_color(rgb(MUTED))
            .child(
                svg()
                    .path(Icon::Forward.path())
                    .size(px(13.0))
                    .text_color(rgb(MUTED)),
            )
            .child(label),
    )
}

/// The bubble's background and corners.
fn frame(bubble: &Bubble) -> Div {
    div()
        .flex()
        .flex_col()
        .rounded(px(14.0))
        .bg(rgb(if bubble.from_me { OUTGOING } else { RAISED }))
        .map(glass)
        .text_size(px(14.5))
        .line_height(relative(1.4))
        .map(|shape| match (bubble.starts_burst, bubble.from_me) {
            // The corner nearest the sender stays sharp on a burst's first
            // bubble, pointing at who said it.
            (true, true) => shape.rounded_tr(px(4.0)),
            (true, false) => shape.rounded_tl(px(4.0)),
            _ => shape,
        })
}

/// Asks for `author`'s profile.
fn open_author(
    author: &Author,
    view: &WeakEntity<ConversationView>,
) -> impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut App) + 'static {
    let request = ProfileRequested {
        chat: author.profile.clone(),
        title: author.name.trim_start_matches("~ ").to_owned().into(),
        verified: false,
    };
    let view = view.clone();
    move |_, _, cx| {
        cx.stop_propagation();
        let request = request.clone();
        let _ = view.update(cx, |_, cx| cx.emit(request));
    }
}

/// The quoted message's block; clicking it goes to the quoted message.
fn jump_to_quoted(quote: &QuotePreview, view: &WeakEntity<ConversationView>) -> Stateful<Div> {
    let (id, view) = (quote.id.clone(), view.clone());
    quoted(quote)
        .id(SharedString::from(format!("quote-{id}")))
        .cursor_pointer()
        .hover(|quote| quote.opacity(0.85))
        .active(|quote| quote.opacity(0.7))
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            let _ = view.update(cx, |view, cx| view.go_to_message(&id, cx));
        })
}

/// Who wrote a group message: their name in their color, and their
/// number on the right beside a chosen name. Their name opens their
/// profile.
fn author(bubble: &Bubble, view: &WeakEntity<ConversationView>) -> Option<Div> {
    bubble.author.as_ref().map(|author| {
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .id(SharedString::from(format!("author-{}", bubble.id)))
                    .cursor_pointer()
                    .hover(Styled::underline)
                    .on_click(open_author(author, view))
                    .child(name_and_phone(
                        author.name.clone(),
                        author.color,
                        author.phone.clone(),
                    )),
            )
            .children(
                author
                    .role
                    .clone()
                    .map(|role| div().text_size(px(12.0)).text_color(rgb(MUTED)).child(role)),
            )
    })
}

/// Reply and react buttons, shown while the pointer is over the bubble.
fn hover_actions(bubble: &Bubble, view: &WeakEntity<ConversationView>) -> Div {
    div()
        .flex()
        .invisible()
        .group_hover("bubble", Styled::visible)
        .child(hover_button("react", Icon::Emoji, "React").on_click({
            let (view, id) = (view.clone(), bubble.id.clone());
            move |_, _, cx| {
                let _ = view.update(cx, |view, cx| view.toggle_reactions(&id, cx));
            }
        }))
        .child(hover_button("reply", Icon::Reply, "Reply").on_click({
            let (view, bubble) = (view.clone(), bubble.clone());
            move |_, _, cx| reply_to(&view, &bubble, cx)
        }))
}

fn hover_button(id: &'static str, icon: Icon, name: &'static str) -> Button {
    Button::new(id)
        .icon(icon)
        .variant(ButtonVariant::Ghost)
        .size(ButtonSize::Small)
        .round()
        .tooltip(name)
}

/// The quick reaction strip; this account's current reaction is
/// highlighted and picking it again takes it back.
fn quick_reactions(bubble: &Bubble, view: &WeakEntity<ConversationView>) -> Stateful<Div> {
    div()
        .id("quick-reactions")
        .on_mouse_down_out({
            let view = view.clone();
            move |_, _, cx| {
                let _ = view.update(cx, ConversationView::close_popups);
            }
        })
        .flex()
        .gap_0p5()
        .p_0p5()
        .rounded_full()
        .bg(rgb(PANEL))
        .map(glass)
        .children(QUICK_REACTIONS.iter().map(|&emoji| {
            let mine = bubble.my_reaction.as_deref() == Some(emoji);
            div()
                .id(emoji)
                .size(px(32.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .cursor_pointer()
                .when(mine, |button| button.bg(rgb(RAISED)))
                .hover(|style| style.bg(rgb(RAISED)))
                .on_click({
                    let (view, bubble) = (view.clone(), bubble.clone());
                    move |_, _, cx| {
                        let _ = view.update(cx, |view, cx| view.react(&bubble, emoji, cx));
                    }
                })
                .text_size(px(20.0))
                .child(emoji)
        }))
        // Any other emoji, from the full picker, as on the phone.
        .child(
            Button::new("more-reactions")
                .icon(Icon::Plus)
                .variant(ButtonVariant::Ghost)
                .size(ButtonSize::Small)
                .round()
                .tooltip("More reactions")
                .on_click({
                    let (view, id) = (view.clone(), bubble.id.clone());
                    move |event, _, cx| {
                        let position = event.position();
                        let _ = view.update(cx, |view, cx| {
                            view.open_reaction_picker(id.clone(), position, cx);
                        });
                    }
                }),
        )
}

fn reaction_chips(bubble: &Bubble) -> Div {
    div()
        .flex()
        .gap_1()
        .children(bubble.reactions.iter().map(|chip| {
            div()
                .flex()
                .items_center()
                .gap_1()
                .px_1p5()
                .rounded_full()
                .bg(rgb(PANEL))
                .border_1()
                .border_color(rgb(LINE))
                .text_size(px(13.0))
                .child(chip.emoji.clone())
                .when(chip.count > 1, |chip_view| {
                    chip_view.child(div().text_color(rgb(MUTED)).child(chip.count.to_string()))
                })
        }))
}

fn reply_to(view: &WeakEntity<ConversationView>, bubble: &Bubble, cx: &mut App) {
    // The view is gone only while the window closes.
    let _ = view.update(cx, |view, cx| view.reply_to(bubble, cx));
}

/// Where a sent message stands, as WhatsApp shows it: a clock while
/// sending, one check when sent, two when delivered, tinted once read.
fn delivery_mark(delivery: Delivery) -> AnyElement {
    let (icon, color) = match delivery {
        Delivery::Failed => {
            return div()
                .flex()
                .items_center()
                .gap_1()
                .text_color(rgb(ALERT))
                .child(
                    svg()
                        .path(Icon::Alert.path())
                        .size(px(13.0))
                        .text_color(rgb(ALERT)),
                )
                .child("Not sent")
                .into_any_element();
        }
        Delivery::Sending => (Icon::Clock, MUTED),
        Delivery::Sent => (Icon::Check, MUTED),
        Delivery::Delivered => (Icon::Checks, MUTED),
        Delivery::Read => (Icon::Checks, MINT),
    };
    svg()
        .path(icon.path())
        .size(px(15.0))
        .text_color(rgb(color))
        .into_any_element()
}
