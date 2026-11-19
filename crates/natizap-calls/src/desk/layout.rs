//! Where the call under way shows in the main window, as the user last
//! arranged it: a bar only, docked over the chat, filling the chat's area,
//! or floating as a small picture. Chats stay beside it in all of them.

use gpui::{Context, Pixels, Size, px, size};

use super::CallDesk;

/// The docked call's height when the user has not chosen one.
const DOCKED_HEIGHT: f32 = 320.0;
/// The floating picture's size when the user has not chosen one.
const FLOATING_SIZE: (f32, f32) = (340.0, 220.0);

/// How the call shows in the main window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallLayout {
    /// Only the bar across the top of the window.
    Bar,
    /// Over the chat, which goes on below it.
    Docked,
    /// Over the whole of the chat's area.
    Filled,
    /// A small picture floating over the window.
    Floating,
}

/// The user's arrangement of the call, kept from one call to the next.
#[derive(Debug, Clone, Copy)]
pub(super) struct Arrangement {
    pub(super) layout: CallLayout,
    pub(super) docked_height: Pixels,
    pub(super) floating_size: Size<Pixels>,
    /// The call fills the whole screen. Unlike the rest, it lasts only
    /// for the call it was chosen in.
    pub(super) full_screen: bool,
}

impl Default for Arrangement {
    fn default() -> Self {
        Self {
            layout: CallLayout::Bar,
            docked_height: px(DOCKED_HEIGHT),
            floating_size: size(px(FLOATING_SIZE.0), px(FLOATING_SIZE.1)),
            full_screen: false,
        }
    }
}

impl CallDesk {
    /// How the call shows in the main window.
    #[must_use]
    pub fn layout(&self) -> CallLayout {
        self.arrangement.layout
    }

    /// Shows the call as `layout` says.
    pub fn set_layout(&mut self, layout: CallLayout, cx: &mut Context<Self>) {
        self.arrangement.layout = layout;
        cx.notify();
    }

    /// Whether the call fills the whole screen, in place of the window's
    /// content.
    #[must_use]
    pub fn full_screen(&self) -> bool {
        self.arrangement.full_screen && self.call.is_some()
    }

    /// Fills the whole screen with the call, or (`false`) gives the window
    /// its content back.
    pub fn set_full_screen(&mut self, full: bool, cx: &mut Context<Self>) {
        self.arrangement.full_screen = full;
        cx.notify();
    }

    /// Where the user put the ringing card, as fractions of the room it
    /// has across and down: the middle until they move it.
    #[must_use]
    pub fn ring_card_place(&self) -> (f32, f32) {
        self.choices.ring_card.unwrap_or((0.5, 0.5))
    }

    /// Keeps the ringing card where the user put it, for the next calls
    /// and the next runs.
    pub fn set_ring_card_place(&mut self, place: (f32, f32), cx: &mut Context<Self>) {
        self.choices.ring_card = Some((place.0.clamp(0.0, 1.0), place.1.clamp(0.0, 1.0)));
        self.choices.write(self.settings.clone(), cx);
    }

    /// The docked call's height.
    #[must_use]
    pub fn docked_height(&self) -> Pixels {
        self.arrangement.docked_height
    }

    /// Docks the call `height` tall.
    pub fn set_docked_height(&mut self, height: Pixels, cx: &mut Context<Self>) {
        self.arrangement.docked_height = height;
        cx.notify();
    }

    /// The floating picture's size.
    #[must_use]
    pub fn floating_size(&self) -> Size<Pixels> {
        self.arrangement.floating_size
    }

    /// Floats the call's picture at `size`.
    pub fn set_floating_size(&mut self, size: Size<Pixels>, cx: &mut Context<Self>) {
        self.arrangement.floating_size = size;
        cx.notify();
    }

    /// A call gained video: one showing as a bar only docks, for its
    /// pictures to be seen.
    pub(super) fn show_video(&mut self) {
        if self.arrangement.layout == CallLayout::Bar {
            self.arrangement.layout = CallLayout::Docked;
        }
    }

    /// A call starts: a voice call shows as a bar, a video call docked,
    /// unless the user moved calls elsewhere.
    pub(super) fn arrange_new_call(&mut self, video: bool) {
        let layout = &mut self.arrangement.layout;
        *layout = match (*layout, video) {
            (CallLayout::Filled | CallLayout::Floating, true) => *layout,
            (_, true) => CallLayout::Docked,
            (_, false) => CallLayout::Bar,
        };
    }
}
