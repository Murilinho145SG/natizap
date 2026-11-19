//! Messages businesses send with buttons under their text, and the replies
//! people send by tapping one. WhatsApp has four kinds of them over the
//! years (buttons, templates, interactive and list messages); they read
//! here as one: a text, maybe a footer, and the buttons.

use whatsapp_rust::prelude::wa;

use crate::answer::Answer;

/// The buttons under a business's message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Buttons {
    /// The small line under the text.
    pub footer: Option<String>,
    /// The buttons, in order.
    pub buttons: Vec<Button>,
}

/// One button.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Button {
    /// What it says.
    pub label: String,
    /// What tapping it does.
    pub action: ButtonAction,
}

/// What a button does when tapped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ButtonAction {
    /// Answers with its own label, as a quick reply, naming the button.
    Reply(Answer),
    /// Opens a web page.
    Url(String),
    /// Calls a phone number.
    Call(String),
    /// Copies a code.
    Copy(String),
    /// Opens a list of choices.
    List,
}

/// The text and buttons of `message`, if it is a message with buttons.
pub(crate) fn of(message: &wa::Message) -> Option<(Option<String>, Buttons)> {
    if let Some(buttons) = message.buttons_message.as_option() {
        return Some(buttons_message(buttons));
    }
    if let Some(template) = message.template_message.as_option() {
        return template_message(template);
    }
    if let Some(interactive) = message.interactive_message.as_option() {
        return interactive_message(interactive);
    }
    let list = message.list_message.as_option()?;
    let text = join_title(list.title.clone(), list.description.clone());
    Some((
        text,
        Buttons {
            footer: non_empty(list.footer_text.clone()),
            buttons: vec![Button {
                label: list.button_text.clone().unwrap_or_else(|| "Options".into()),
                action: ButtonAction::List,
            }],
        },
    ))
}

/// The text of a reply someone sent by tapping a button or choosing from
/// a list: the label they chose.
pub(crate) fn reply_text(message: &wa::Message) -> Option<String> {
    use wa::__buffa::oneof::message::buttons_response_message::Response;
    let chosen = message
        .buttons_response_message
        .as_option()
        .and_then(|reply| {
            reply
                .response
                .as_ref()
                .map(|Response::SelectedDisplayText(text)| text.clone())
        })
        .or_else(|| {
            message
                .template_button_reply_message
                .as_option()
                .and_then(|reply| reply.selected_display_text.clone())
        })
        .or_else(|| {
            message
                .list_response_message
                .as_option()
                .and_then(|reply| reply.title.clone())
        })
        .or_else(|| {
            message
                .interactive_response_message
                .as_option()
                .and_then(|reply| reply.body.as_option())
                .and_then(|body| body.text.clone())
        });
    non_empty(chosen)
}

/// The reply metadata of the kinds this module reads, wherever each keeps
/// it.
pub(crate) fn context_info(message: &wa::Message) -> Option<&wa::ContextInfo> {
    [
        message.buttons_message.as_option().map(|m| &m.context_info),
        message
            .template_message
            .as_option()
            .map(|m| &m.context_info),
        message
            .interactive_message
            .as_option()
            .map(|m| &m.context_info),
        message.list_message.as_option().map(|m| &m.context_info),
        message
            .buttons_response_message
            .as_option()
            .map(|m| &m.context_info),
        message
            .template_button_reply_message
            .as_option()
            .map(|m| &m.context_info),
        message
            .list_response_message
            .as_option()
            .map(|m| &m.context_info),
        message
            .interactive_response_message
            .as_option()
            .map(|m| &m.context_info),
    ]
    .into_iter()
    .flatten()
    .find_map(|info| info.as_option())
}

fn buttons_message(message: &wa::message::ButtonsMessage) -> (Option<String>, Buttons) {
    use wa::__buffa::oneof::message::buttons_message::Header;
    let title = match &message.header {
        Some(Header::Text(title)) => Some(title.clone()),
        _ => None,
    };
    let buttons = message
        .buttons
        .iter()
        .filter_map(|button| {
            if let Some(flow) = button.native_flow_info.as_option() {
                return native_flow(flow.name.as_deref(), flow.params_json.as_deref());
            }
            let label = button.button_text.as_option()?.display_text.clone()?;
            let answer = button
                .button_id
                .clone()
                .map_or(Answer::Unknown, Answer::Button);
            Some(Button {
                label,
                action: ButtonAction::Reply(answer),
            })
        })
        .collect();
    (
        join_title(title, message.content_text.clone()),
        Buttons {
            footer: non_empty(message.footer_text.clone()),
            buttons,
        },
    )
}

fn template_message(message: &wa::message::TemplateMessage) -> Option<(Option<String>, Buttons)> {
    use wa::__buffa::oneof::message::template_message::Format;
    match &message.format {
        Some(Format::HydratedFourRowTemplate(template)) => Some(hydrated(template)),
        Some(Format::InteractiveMessageTemplate(interactive)) => interactive_message(interactive),
        Some(Format::FourRowTemplate(_)) | None => {
            message.hydrated_template.as_option().map(hydrated)
        }
    }
}

fn hydrated(
    template: &wa::message::template_message::HydratedFourRowTemplate,
) -> (Option<String>, Buttons) {
    use wa::__buffa::oneof::hydrated_template_button::HydratedButton;
    use wa::__buffa::oneof::message::template_message::hydrated_four_row_template::Title;
    let title = match &template.title {
        Some(Title::HydratedTitleText(title)) => Some(title.clone()),
        _ => None,
    };
    let buttons = template
        .hydrated_buttons
        .iter()
        .filter_map(|button| {
            let (label, action) = match button.hydrated_button.as_ref()? {
                HydratedButton::QuickReplyButton(reply) => {
                    let answer = reply
                        .id
                        .clone()
                        .map_or(Answer::Unknown, |id| Answer::Template {
                            id,
                            index: button.index.unwrap_or_default(),
                        });
                    (reply.display_text.clone()?, ButtonAction::Reply(answer))
                }
                HydratedButton::UrlButton(link) => (
                    link.display_text.clone()?,
                    ButtonAction::Url(link.url.clone()?),
                ),
                HydratedButton::CallButton(call) => (
                    call.display_text.clone()?,
                    ButtonAction::Call(call.phone_number.clone()?),
                ),
            };
            Some(Button { label, action })
        })
        .collect();
    (
        join_title(title, template.hydrated_content_text.clone()),
        Buttons {
            footer: non_empty(template.hydrated_footer_text.clone()),
            buttons,
        },
    )
}

fn interactive_message(
    message: &wa::message::InteractiveMessage,
) -> Option<(Option<String>, Buttons)> {
    use wa::__buffa::oneof::message::interactive_message::InteractiveMessage;
    let Some(InteractiveMessage::NativeFlowMessage(flow)) = &message.interactive_message else {
        return None;
    };
    let title = message
        .header
        .as_option()
        .and_then(|header| header.title.clone());
    let body = message.body.as_option().and_then(|body| body.text.clone());
    let buttons = flow
        .buttons
        .iter()
        .filter_map(|button| {
            native_flow(button.name.as_deref(), button.button_params_json.as_deref())
        })
        .collect();
    Some((
        join_title(title, body),
        Buttons {
            footer: non_empty(
                message
                    .footer
                    .as_option()
                    .and_then(|footer| footer.text.clone()),
            ),
            buttons,
        },
    ))
}

/// A "native flow" button: its kind is `name`, and what it says and does
/// sit in the JSON `params`.
fn native_flow(name: Option<&str>, params: Option<&str>) -> Option<Button> {
    let params: serde_json::Value = serde_json::from_str(params?).ok()?;
    let field = |key: &str| {
        params
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    };
    let label = field("display_text").or_else(|| field("title"))?;
    let action = match name? {
        "quick_reply" => ButtonAction::Reply(field("id").map_or(Answer::Unknown, Answer::Flow)),
        "cta_url" => ButtonAction::Url(field("url")?),
        "cta_call" => ButtonAction::Call(field("phone_number")?),
        "cta_copy" => ButtonAction::Copy(field("copy_code")?),
        "single_select" => ButtonAction::List,
        // Flows WhatsApp runs inside its own app (payments, forms) have
        // nothing to do here: shown as quick replies would mislead.
        _ => return None,
    };
    Some(Button { label, action })
}

/// The message's text: a bold title over its body, as the phone shows it.
fn join_title(title: Option<String>, body: Option<String>) -> Option<String> {
    match (non_empty(title), non_empty(body)) {
        (Some(title), Some(body)) => Some(format!("*{title}*\n{body}")),
        (title, body) => title.map(|title| format!("*{title}*")).or(body),
    }
}

fn non_empty(text: Option<String>) -> Option<String> {
    text.filter(|text| !text.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_native_flow_buttons_by_their_kind() {
        let reply = native_flow(
            Some("quick_reply"),
            Some(r#"{"display_text":"Sim","id":"yes"}"#),
        );
        assert_eq!(
            reply,
            Some(Button {
                label: "Sim".into(),
                action: ButtonAction::Reply(Answer::Flow("yes".into()))
            })
        );
        let link = native_flow(
            Some("cta_url"),
            Some(r#"{"display_text":"Site","url":"https://example.com"}"#),
        );
        assert_eq!(
            link.map(|button| button.action),
            Some(ButtonAction::Url("https://example.com".into()))
        );
        assert_eq!(native_flow(Some("payment_info"), Some("{}")), None);
        assert_eq!(native_flow(Some("quick_reply"), Some("not json")), None);
    }

    #[test]
    fn puts_the_title_in_bold_over_the_body() {
        assert_eq!(
            join_title(Some("Pedido".into()), Some("Chegou!".into())),
            Some("*Pedido*\nChegou!".into())
        );
        assert_eq!(
            join_title(None, Some("Chegou!".into())),
            Some("Chegou!".into())
        );
        assert_eq!(join_title(Some(" ".into()), None), None);
    }
}
