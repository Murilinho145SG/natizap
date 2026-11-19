//! The splash over the window while natizap opens: it plays its intro,
//! stays while the app is still loading, then fades into the app.

use natizap_ui::theme::ActiveTheme as _;
use std::time::Instant;

use gpui::prelude::*;
use gpui::{AnyElement, Context, div};
use natizap_session::SessionState;
use natizap_ui::{SPLASH_INTRO, SPLASH_OUT, splash};

use super::Root;

/// Where the splash is in its life.
#[derive(Debug, Clone, Copy)]
pub(super) struct Opening {
    since: Instant,
    /// When it started fading away.
    leaving: Option<Instant>,
}

impl Opening {
    /// The splash, from now; redraws the window once its intro is over, in
    /// case the app was ready all along.
    pub(super) fn start(cx: &mut Context<Root>) -> Self {
        Self::redraw_after(SPLASH_INTRO, cx);
        Self {
            since: Instant::now(),
            leaving: None,
        }
    }

    fn redraw_after(wait: std::time::Duration, cx: &mut Context<Root>) {
        cx.spawn(async move |root, cx| {
            cx.background_executor().timer(wait).await;
            let _ = root.update(cx, |_, cx| cx.notify());
        })
        .detach();
    }
}

impl Root {
    /// Whether there is something to show behind the splash: the chats, or
    /// the pairing screen with its code.
    fn opened(&self, cx: &Context<Self>) -> bool {
        !matches!(self.session.read(cx).state(), SessionState::Starting)
            || !self.chats.read(cx).rows().is_empty()
    }

    /// The splash to lay over the window, while it shows: leaving once the
    /// intro played and the app is ready, gone once it faded.
    pub(super) fn opening_layer(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = *cx.theme();
        let mut opening = self.opening?;
        if let Some(leaving) = opening.leaving {
            if leaving.elapsed() >= SPLASH_OUT {
                self.opening = None;
                return None;
            }
        } else if opening.since.elapsed() >= SPLASH_INTRO && self.opened(cx) {
            opening.leaving = Some(Instant::now());
            self.opening = Some(opening);
            Opening::redraw_after(SPLASH_OUT, cx);
        }
        Some(
            div()
                .absolute()
                .inset_0()
                .occlude()
                .child(splash(opening.leaving.is_some(), theme))
                .into_any_element(),
        )
    }
}
