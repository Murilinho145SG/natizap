//! A call in a window of its own: the call's stage filling it. The window
//! closes when the call leaves the screen.

use gpui::prelude::*;
use gpui::{Context, Entity, FontWeight, Window, div, px, rgb};
use natizap_profiles::ProfilePictures;
use natizap_ui::theme::{ActiveTheme as _, DISPLAY_FONT, base_font};
use natizap_ui::window_frame;

use crate::desk::CallDesk;
use crate::picker::SharePicker;
use crate::stage::{CallStage, Home};

/// The call under way, filling a window.
pub struct CallWindow {
    stage: Entity<CallStage>,
    picker: Entity<SharePicker>,
}

impl std::fmt::Debug for CallWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CallWindow").finish_non_exhaustive()
    }
}

impl CallWindow {
    /// Shows `desk`'s call in `window`, closing it once the call is gone.
    pub fn new(
        desk: Entity<CallDesk>,
        pictures: Entity<ProfilePictures>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe_in(&desk, window, |_, desk, window, cx| {
            if desk.read(cx).call(cx).is_none() {
                window.remove_window();
            }
        })
        .detach();
        let picker = cx.new(|cx| SharePicker::new(desk.clone(), cx));
        let stage = cx.new(|cx| CallStage::at(Home::Window, desk, pictures, cx));
        Self { stage, picker }
    }
}

impl Render for CallWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        let title = div()
            .font_family(DISPLAY_FONT)
            .font_weight(FontWeight::BOLD)
            .text_size(px(15.0))
            .child("Call");
        div()
            .size_full()
            .font(base_font())
            .text_color(rgb(theme.text))
            .child(window_frame(window, title, self.stage.clone(), theme))
            .child(self.picker.clone())
    }
}
