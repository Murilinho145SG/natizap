//! A call's bubble in its chat, as the phone draws it: which way it went
//! in a round badge, voice or video, and how long it lasted or how it
//! ended. Clicking it calls back the same way.

use gpui::prelude::*;
use gpui::{Div, FontWeight, WeakEntity, div, px, rgb, svg};
use natizap_protocol::{CallDetails, CallOutcome};
use natizap_ui::Icon;
use natizap_ui::theme::Theme;

use crate::view::ConversationView;

/// The call's badge, title and line under it.
pub(crate) fn call_body(
    id: &str,
    video: bool,
    details: CallDetails,
    view: &WeakEntity<ConversationView>,
    theme: Theme,
) -> Div {
    let (icon, color) = badge(video, details, theme);
    let view = view.clone();
    div().child(
        div()
            .id(gpui::SharedString::from(format!("call-{id}")))
            .min_w(px(200.0))
            .flex()
            .items_center()
            .gap_3()
            .py_1()
            .cursor_pointer()
            .on_click(move |_, _, cx| {
                let _ = view.update(cx, |view, cx| view.call_back(video, cx));
            })
            .child(
                div()
                    .size(px(40.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(rgb(theme.deep))
                    .child(
                        svg()
                            .path(icon.path())
                            .size(px(18.0))
                            .text_color(rgb(color)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(if video {
                        "Video call"
                    } else {
                        "Voice call"
                    }))
                    .child(
                        div()
                            .text_size(px(13.0))
                            .text_color(rgb(theme.muted))
                            .child(line(details)),
                    ),
            ),
    )
}

/// The badge's icon and colour: which way the call went, and in the
/// alert colour when it came in and nobody took it.
fn badge(video: bool, details: CallDetails, theme: Theme) -> (Icon, u32) {
    let icon = match (video, details.incoming) {
        (false, true) => Icon::CallIncoming,
        (false, false) => Icon::CallOutgoing,
        (true, true) => Icon::VideoIncoming,
        (true, false) => Icon::VideoOutgoing,
    };
    let unanswered = matches!(details.outcome, CallOutcome::Missed | CallOutcome::Declined);
    let color = if details.incoming && unanswered {
        theme.alert
    } else {
        theme.muted
    };
    (icon, color)
}

/// What the line under the title says: how long it lasted, or how it
/// ended.
fn line(details: CallDetails) -> String {
    match (details.outcome, details.incoming, details.seconds) {
        (CallOutcome::Answered, _, Some(seconds)) => length(seconds),
        (CallOutcome::Answered, true, None) => "Answered".into(),
        (CallOutcome::Answered, false, None) => "Outgoing".into(),
        (CallOutcome::AnsweredElsewhere, ..) => "Answered on another device".into(),
        (CallOutcome::Missed, true, _) => "Missed".into(),
        (CallOutcome::Missed, false, _) => "No answer".into(),
        (CallOutcome::Declined, ..) => "Declined".into(),
    }
}

/// How long a call lasted, as the phone says it: `31 s`, `4 min`, `1 h 5 min`.
fn length(seconds: u32) -> String {
    match seconds {
        0..60 => format!("{seconds} s"),
        60..3600 => format!("{} min", seconds / 60),
        _ => match (seconds / 3600, seconds % 3600 / 60) {
            (hours, 0) => format!("{hours} h"),
            (hours, minutes) => format!("{hours} h {minutes} min"),
        },
    }
}

#[cfg(test)]
mod tests {
    use natizap_protocol::{CallDetails, CallOutcome};

    use super::{length, line};

    fn call(incoming: bool, outcome: CallOutcome, seconds: Option<u32>) -> String {
        line(CallDetails {
            incoming,
            outcome,
            seconds,
        })
    }

    #[test]
    fn says_how_long_it_lasted_or_how_it_ended() {
        assert_eq!(call(true, CallOutcome::Answered, Some(31)), "31 s");
        assert_eq!(
            call(true, CallOutcome::AnsweredElsewhere, None),
            "Answered on another device"
        );
        assert_eq!(call(true, CallOutcome::Missed, None), "Missed");
        assert_eq!(call(false, CallOutcome::Missed, None), "No answer");
    }

    #[test]
    fn a_length_reads_as_the_phone_says_it() {
        assert_eq!(length(59), "59 s");
        assert_eq!(length(240), "4 min");
        assert_eq!(length(3900), "1 h 5 min");
        assert_eq!(length(7200), "2 h");
    }
}
