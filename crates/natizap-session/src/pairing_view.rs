use gpui::prelude::*;
use gpui::{Context, Div, Entity, SharedString, Window, div, px, rgb};

use crate::{QrMatrix, Session, SessionState};

/// Side length of the rendered QR code, quiet zone excluded.
const QR_SIZE: f32 = 264.0;
/// Light border around the code; scanners need it to find the edges.
const QR_QUIET_ZONE: f32 = 16.0;

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
            SessionState::Starting => status("Connecting to WhatsApp…"),
            SessionState::AwaitingPairing(qr) => pairing(qr),
            SessionState::PairingExpired => {
                status("The code expired before it was scanned.").child(Self::retry_button(cx))
            }
            SessionState::Paired => status("Linked. Finishing setup…"),
            SessionState::Connected => status("Connected"),
            SessionState::Reconnecting => status("Connection lost. Reconnecting…"),
            SessionState::LoggedOut => {
                status("This device was unlinked from your phone.").child(Self::retry_button(cx))
            }
            SessionState::Failed(error) => status(error.to_string()).child(Self::retry_button(cx)),
        };
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgb(0x0b_14_1a))
            .text_color(rgb(0xe9_ed_ef))
            .child(body)
    }
}

impl PairingView {
    fn retry_button(cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("retry")
            .px_4()
            .py_2()
            .rounded_md()
            .bg(rgb(0x00_a8_84))
            .text_color(rgb(0x0b_14_1a))
            .cursor_pointer()
            .hover(|style| style.bg(rgb(0x06_cf_9c)))
            .on_click(cx.listener(|this, _, _, cx| {
                this.session.update(cx, Session::restart);
            }))
            .child("Try again")
    }
}

fn status(message: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap_4()
        .text_lg()
        .child(message.into())
}

fn pairing(qr: &QrMatrix) -> Div {
    div()
        .flex()
        .gap(px(48.0))
        .items_center()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_3()
                .max_w(px(380.0))
                .child(div().text_2xl().child("Link natizap to your phone"))
                .child("1. Open WhatsApp on your phone")
                .child("2. Go to Settings › Linked devices")
                .child("3. Tap Link a device and scan this code"),
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
        .p(px(QR_QUIET_ZONE))
        .bg(rgb(0xff_ff_ff))
        .rounded_md()
        .children(qr.rows().map(|row| {
            div().flex().children(
                row.iter()
                    .map(|&dark| div().size(module).when(dark, |m| m.bg(rgb(0x00_00_00)))),
            )
        }))
}
