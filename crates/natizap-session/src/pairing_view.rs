use gpui::prelude::*;
use gpui::{Context, Div, Entity, FontWeight, SharedString, Window, div, px, relative, rgb, svg};
use natizap_ui::Icon;
use natizap_ui::theme::{DEEP, DISPLAY_FONT, HONEY, HONEY_INK, LINE, MUTED, PANE, TEXT};

use crate::{QrMatrix, Session, SessionState};

/// Side length of the rendered QR code, quiet zone excluded.
const QR_SIZE: f32 = 264.0;
/// Light border around the code; scanners need it to find the edges.
const QR_QUIET_ZONE: f32 = 20.0;

/// The screen shown until the session is connected: progress, the pairing QR
/// code, or why the session stopped with a way to try again.
#[derive(Debug)]
pub struct PairingView {
    session: Entity<Session>,
}

impl PairingView {
    /// Creates a view that re-renders whenever `session` changes.
    pub fn new(session: Entity<Session>, cx: &mut Context<Self>) -> Self {
        cx.observe(&session, |_, _, cx| cx.notify()).detach();
        Self { session }
    }
}

impl Render for PairingView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match self.session.read(cx).state() {
            SessionState::AwaitingPairing(qr) => pairing(qr),
            SessionState::Starting => notice("Connecting…", "Reaching WhatsApp."),
            SessionState::Paired => notice("Linked", "Bringing your chats over from your phone…"),
            SessionState::Connected => notice("Connected", "Opening your chats…"),
            SessionState::Reconnecting => {
                notice("Reconnecting…", "The connection dropped. natizap keeps trying.")
            }
            SessionState::PairingExpired => notice(
                "The code expired",
                "Codes refresh for a few minutes, then stop until you ask for a new one.",
            )
            .child(Self::restart_button("Show a new code", cx)),
            SessionState::LoggedOut => notice(
                "This computer was unlinked",
                "It was removed from Linked devices on your phone. Link it again to keep using natizap.",
            )
            .child(Self::restart_button("Link again", cx)),
            SessionState::Failed(error) => {
                notice("natizap could not connect", error.to_string())
                    .child(Self::restart_button("Try again", cx))
            }
        };
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .p_8()
            .bg(rgb(PANE))
            .text_color(rgb(TEXT))
            .child(body)
    }
}

impl PairingView {
    fn restart_button(label: &'static str, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("restart")
            .mt_2()
            .px_5()
            .py_2p5()
            .rounded_full()
            .bg(rgb(HONEY))
            .text_color(rgb(HONEY_INK))
            .font_weight(FontWeight::SEMIBOLD)
            .cursor_pointer()
            .hover(|style| style.opacity(0.9))
            .on_click(cx.listener(|this, _, _, cx| {
                this.session.update(cx, Session::restart);
            }))
            .child(label)
    }
}

fn notice(title: &'static str, detail: impl Into<SharedString>) -> Div {
    div()
        .max_w(px(420.0))
        .flex()
        .flex_col()
        .items_center()
        .gap_2()
        .text_center()
        .child(heading(title).text_size(px(24.0)))
        .child(div().text_color(rgb(MUTED)).child(detail.into()))
}

fn heading(text: &'static str) -> Div {
    div()
        .font_family(DISPLAY_FONT)
        .font_weight(FontWeight::BOLD)
        .child(text)
}

fn pairing(qr: &QrMatrix) -> Div {
    let steps = [
        "Open WhatsApp on your phone",
        "Go to Settings, then Linked devices",
        "Tap Link a device and point the camera here",
    ];
    div()
        .flex()
        .items_center()
        .gap(px(56.0))
        .child(
            div()
                .max_w(px(360.0))
                .flex()
                .flex_col()
                .gap_6()
                .child(
                    heading("Link natizap to your phone")
                        .text_size(px(32.0))
                        .line_height(relative(1.1)),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_4()
                        .children(steps.iter().enumerate().map(|(index, step)| {
                            div()
                                .flex()
                                .items_center()
                                .gap_3()
                                .child(
                                    div()
                                        .size(px(26.0))
                                        .flex_none()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded_full()
                                        .border_1()
                                        .border_color(rgb(LINE))
                                        .text_size(px(13.0))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(rgb(HONEY))
                                        .child((index + 1).to_string()),
                                )
                                .child(*step)
                        })),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .pt_2()
                        .text_size(px(13.0))
                        .text_color(rgb(MUTED))
                        .child(
                            svg()
                                .path(Icon::Lock.path())
                                .size(px(14.0))
                                .text_color(rgb(MUTED)),
                        )
                        .child("Messages stay end-to-end encrypted and are kept on this computer."),
                ),
        )
        .child(qr_code(qr))
}

#[expect(
    clippy::cast_precision_loss,
    reason = "QR codes are at most 177 modules wide, exactly representable in f32"
)]
fn qr_code(qr: &QrMatrix) -> Div {
    let module = px(QR_SIZE / qr.width() as f32);
    div()
        .flex_none()
        .p(px(QR_QUIET_ZONE))
        .bg(rgb(0xff_ff_ff))
        .rounded_2xl()
        .children(qr.rows().map(|row| {
            div().flex().children(
                row.iter()
                    .map(|&dark| div().size(module).when(dark, |m| m.bg(rgb(DEEP)))),
            )
        }))
}
