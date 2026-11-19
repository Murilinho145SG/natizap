//! Tapping a business's quick-reply button: the answer names the button,
//! so the business's bot knows which one was chosen, and shows its label
//! as a reply to the message.

use std::sync::Arc;

use whatsapp_rust::Jid;
use whatsapp_rust::prelude::{Client, MessageBuilderExt as _, MessageExt as _, MessageField, wa};

use crate::actions::{ActionError, Reply, Sent, reply_context};

/// Which quick-reply button was tapped, in the words of the message that
/// offered it: WhatsApp answers each kind of button message differently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// A button of a buttons message, by its id.
    Button(String),
    /// A button of a template, by its id and place.
    Template { id: String, index: u32 },
    /// A "native flow" quick reply, by its id.
    Flow(String),
    /// A button kept before its id was; answered with its label alone.
    Unknown,
}

impl Answer {
    /// How the message store spells it.
    #[must_use]
    pub fn key(&self) -> String {
        match self {
            Self::Button(id) => format!("b:{id}"),
            Self::Template { id, index } => format!("t{index}:{id}"),
            Self::Flow(id) => format!("f:{id}"),
            Self::Unknown => String::new(),
        }
    }

    /// The answer spelled `key`; anything unreadable is [`Answer::Unknown`].
    #[must_use]
    pub fn from_key(key: &str) -> Self {
        let Some((kind, id)) = key.split_once(':') else {
            return Self::Unknown;
        };
        let id = id.to_owned();
        match kind {
            "b" => Self::Button(id),
            "f" => Self::Flow(id),
            _ => kind
                .strip_prefix('t')
                .and_then(|index| index.parse().ok())
                .map_or(Self::Unknown, |index| Self::Template { id, index }),
        }
    }

    /// The message that answers with this button, labelled `label`, as a
    /// reply to the message that offered it.
    fn message(&self, label: String, context: wa::ContextInfo) -> wa::Message {
        use wa::__buffa::oneof::message::buttons_response_message::Response;
        use wa::__buffa::oneof::message::interactive_response_message::InteractiveResponseMessage;
        use wa::message::{
            ButtonsResponseMessage, InteractiveResponseMessage as Interactive,
            TemplateButtonReplyMessage, buttons_response_message, interactive_response_message,
        };
        match self {
            Self::Button(id) => wa::Message {
                buttons_response_message: MessageField::some(ButtonsResponseMessage {
                    selected_button_id: Some(id.clone()),
                    response: Some(Response::SelectedDisplayText(label)),
                    r#type: Some(buttons_response_message::Type::DISPLAY_TEXT),
                    context_info: MessageField::some(context),
                }),
                ..Default::default()
            },
            Self::Template { id, index } => wa::Message {
                template_button_reply_message: MessageField::some(TemplateButtonReplyMessage {
                    selected_id: Some(id.clone()),
                    selected_display_text: Some(label),
                    selected_index: Some(*index),
                    context_info: MessageField::some(context),
                    ..Default::default()
                }),
                ..Default::default()
            },
            Self::Flow(id) => wa::Message {
                interactive_response_message: MessageField::some(Interactive {
                    body: MessageField::some(interactive_response_message::Body {
                        text: Some(label),
                        ..Default::default()
                    }),
                    interactive_response_message: Some(
                        InteractiveResponseMessage::NativeFlowResponseMessage(Box::new(
                            interactive_response_message::NativeFlowResponseMessage {
                                name: Some("quick_reply".into()),
                                params_json: Some(serde_json::json!({ "id": id }).to_string()),
                                version: Some(3),
                            },
                        )),
                    ),
                    context_info: MessageField::some(context),
                }),
                ..Default::default()
            },
            Self::Unknown => {
                let mut text = wa::Message::text(label);
                text.set_context_info(context);
                text
            }
        }
    }
}

/// Answers `offered`, the message with the button, by tapping `answer`.
pub(crate) async fn send(
    client: Arc<Client>,
    to: Jid,
    label: String,
    answer: Answer,
    offered: Reply,
) -> Result<Sent, ActionError> {
    let context = reply_context(&client, offered)?;
    let sent = client
        .send_message(to, answer.message(label, context))
        .await
        .map_err(|error| ActionError::refused(&error))?;
    Ok(Sent {
        id: sent.message_id,
        secret: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spells_answers_both_ways() {
        for answer in [
            Answer::Button("yes".into()),
            Answer::Template {
                id: "buy:1".into(),
                index: 2,
            },
            Answer::Flow("menu".into()),
        ] {
            assert_eq!(Answer::from_key(&answer.key()), answer);
        }
        assert_eq!(Answer::from_key(""), Answer::Unknown);
    }
}
