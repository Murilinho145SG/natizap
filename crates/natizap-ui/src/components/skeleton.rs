//! A placeholder shaped like content that is still loading.

use std::time::Duration;

use gpui::prelude::*;
use gpui::{
    Animation, AnimationElement, AnimationExt as _, Div, ElementId, pulsating_between, rgb,
};

use crate::theme::RAISED;

/// `shape`, sized and rounded by the caller, filled and gently pulsing.
/// `id` must be unique among skeletons on screen so each keeps its pulse.
pub fn skeleton(id: impl Into<ElementId>, shape: Div) -> AnimationElement<Div> {
    shape.bg(rgb(RAISED)).with_animation(
        id,
        Animation::new(Duration::from_millis(1400))
            .repeat()
            .with_easing(pulsating_between(0.45, 1.0)),
        Styled::opacity,
    )
}
