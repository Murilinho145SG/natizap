//! The dialog that writes a poll: its question, its choices and whether
//! several may be chosen.

use gpui::prelude::*;
use gpui::{Context, Div, Entity, EventEmitter, Focusable as _, FontWeight, Window, div, px, rgb};
use natizap_protocol::Poll;
use natizap_ui::theme::{LINE, MUTED, PANEL, TEXT, glass};
use natizap_ui::{Button, ButtonVariant, Switch, TextInput};

/// WhatsApp allows at most this many choices.
const MOST_OPTIONS: usize = 12;

/// The poll is written and should be sent.
#[derive(Debug, Clone)]
pub(crate) struct PollWritten(pub(crate) Poll);

/// The user gave up on the poll.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PollDropped;

/// A poll being written.
#[derive(Debug)]
pub(crate) struct PollComposer {
    question: Entity<TextInput>,
    options: Vec<Entity<TextInput>>,
    multiple: bool,
}

impl EventEmitter<PollWritten> for PollComposer {}
impl EventEmitter<PollDropped> for PollComposer {}

impl PollComposer {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let question = cx.new(|cx| TextInput::new("Ask a question", cx));
        cx.observe(&question, |_, _, cx| cx.notify()).detach();
        window.focus(&question.focus_handle(cx), cx);
        let mut composer = Self {
            question,
            options: Vec::new(),
            multiple: true,
        };
        composer.add_option(cx);
        composer.add_option(cx);
        composer
    }

    /// An empty choice at the end. Typing in the last one adds the next,
    /// as on the phone.
    fn add_option(&mut self, cx: &mut Context<Self>) {
        let option = cx.new(|cx| TextInput::new("Add option", cx));
        cx.observe(&option, |composer, option, cx| {
            let last = composer.options.last() == Some(&option);
            if last
                && !option.read(cx).text().trim().is_empty()
                && composer.options.len() < MOST_OPTIONS
            {
                composer.add_option(cx);
            }
            cx.notify();
        })
        .detach();
        self.options.push(option);
    }

    /// The poll, once it has a question and two choices.
    fn poll(&self, cx: &Context<Self>) -> Option<Poll> {
        let question = self.question.read(cx).text().trim().to_owned();
        let options: Vec<String> = self
            .options
            .iter()
            .map(|option| option.read(cx).text().trim().to_owned())
            .filter(|option| !option.is_empty())
            .collect();
        (!question.is_empty() && options.len() >= 2).then_some(Poll {
            question,
            options,
            multiple: self.multiple,
        })
    }
}

impl Render for PollComposer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ready = self.poll(cx);
        let composer = cx.weak_entity();
        glass(
            div()
                .id("poll-composer")
                .w(px(400.0))
                .max_h(px(560.0))
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap_3()
                .p_5()
                .rounded_xl()
                .bg(rgb(PANEL))
                .text_color(rgb(TEXT))
                // Clicks inside must not reach the scrim, which closes it.
                .on_click(|_, _, cx| cx.stop_propagation()),
        )
        .child(
            div()
                .text_size(px(16.0))
                .font_weight(FontWeight::SEMIBOLD)
                .child("Create poll"),
        )
        .child(label("Question"))
        .child(field(&self.question))
        .child(label("Options"))
        .children(self.options.iter().map(field))
        .child(
            div()
                .pt_1()
                .flex()
                .items_center()
                .justify_between()
                .child(div().text_size(px(14.0)).child("Allow multiple answers"))
                .child(
                    Switch::new("poll-multiple", self.multiple).on_toggle(move |on, _, cx| {
                        let _ = composer.update(cx, |composer, cx| {
                            composer.multiple = on;
                            cx.notify();
                        });
                    }),
                ),
        )
        .child(
            div()
                .pt_2()
                .flex()
                .justify_end()
                .gap_2()
                .child(
                    Button::new("poll-cancel")
                        .label("Cancel")
                        .variant(ButtonVariant::Secondary)
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(PollDropped))),
                )
                .child(
                    Button::new("poll-send")
                        .label("Send")
                        .variant(if ready.is_some() {
                            ButtonVariant::Primary
                        } else {
                            ButtonVariant::Secondary
                        })
                        .on_click(cx.listener(|composer, _, _, cx| {
                            if let Some(poll) = composer.poll(cx) {
                                cx.emit(PollWritten(poll));
                            }
                        })),
                ),
        )
    }
}

fn label(text: &'static str) -> Div {
    div().text_size(px(12.5)).text_color(rgb(MUTED)).child(text)
}

fn field(input: &Entity<TextInput>) -> Div {
    div()
        .px_3()
        .py_2()
        .rounded_lg()
        .border_1()
        .border_color(rgb(LINE))
        .child(input.clone())
}
