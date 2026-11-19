//! How a message's shared contacts, poll or buttons are kept in the
//! `extra` column: one line per entry, fields split by tabs.

use natizap_protocol::{Answer, Button, ButtonAction, Buttons, Card, Extra, Poll};

pub(super) fn encode(extra: &Extra) -> String {
    let clean = |field: &str| field.replace(['\t', '\n', '\r'], " ");
    match extra {
        Extra::Cards(cards) => cards
            .iter()
            .map(|card| {
                format!(
                    "card\t{}\t{}",
                    clean(&card.name),
                    card.phone.as_deref().unwrap_or_default()
                )
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Extra::Poll(poll) => std::iter::once(format!(
            "poll\t{}\t{}",
            u8::from(poll.multiple),
            clean(&poll.question)
        ))
        .chain(
            poll.options
                .iter()
                .map(|option| format!("option\t{}", clean(option))),
        )
        .collect::<Vec<_>>()
        .join("\n"),
        Extra::Buttons(buttons) => std::iter::once(format!(
            "buttons\t{}",
            clean(buttons.footer.as_deref().unwrap_or_default())
        ))
        .chain(buttons.buttons.iter().map(|button| {
            let (kind, target) = match &button.action {
                ButtonAction::Reply(answer) => ("reply", answer.key()),
                ButtonAction::Url(url) => ("url", url.clone()),
                ButtonAction::Call(phone) => ("call", phone.clone()),
                ButtonAction::Copy(code) => ("copy", code.clone()),
                ButtonAction::List => ("list", String::new()),
            };
            format!(
                "button\t{kind}\t{}\t{}",
                clean(&target),
                clean(&button.label)
            )
        }))
        .collect::<Vec<_>>()
        .join("\n"),
    }
}

pub(super) fn decode(stored: &str) -> Option<Extra> {
    let mut lines = stored
        .lines()
        .map(|line| line.split('\t').collect::<Vec<_>>());
    let first = lines.next()?;
    match first.as_slice() {
        ["poll", multiple, question] => Some(Extra::Poll(Poll {
            question: (*question).to_owned(),
            multiple: *multiple == "1",
            options: lines
                .filter_map(|fields| match fields.as_slice() {
                    ["option", option] => Some((*option).to_owned()),
                    _ => None,
                })
                .collect(),
        })),
        ["buttons", footer] => Some(Extra::Buttons(Buttons {
            footer: Some((*footer).to_owned()).filter(|footer| !footer.is_empty()),
            buttons: lines
                .filter_map(|fields| match fields.as_slice() {
                    ["button", kind, target, label] => {
                        let target = (*target).to_owned();
                        let action = match *kind {
                            "reply" => ButtonAction::Reply(Answer::from_key(&target)),
                            "url" => ButtonAction::Url(target),
                            "call" => ButtonAction::Call(target),
                            "copy" => ButtonAction::Copy(target),
                            "list" => ButtonAction::List,
                            _ => return None,
                        };
                        Some(Button {
                            label: (*label).to_owned(),
                            action,
                        })
                    }
                    _ => None,
                })
                .collect(),
        })),
        ["card", ..] => Some(Extra::Cards(
            std::iter::once(first)
                .chain(lines)
                .filter_map(|fields| match fields.as_slice() {
                    ["card", name, phone] => Some(Card {
                        name: (*name).to_owned(),
                        phone: Some((*phone).to_owned()).filter(|phone| !phone.is_empty()),
                    }),
                    _ => None,
                })
                .collect(),
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_cards_and_polls_as_they_were() {
        let cards = Extra::Cards(vec![
            Card {
                name: "Ana\tSouza".into(),
                phone: Some("5511912345678".into()),
            },
            Card {
                name: "Bia".into(),
                phone: None,
            },
        ]);
        let Some(Extra::Cards(read)) = decode(&encode(&cards)) else {
            panic!("cards must read back");
        };
        assert_eq!(read[0].name, "Ana Souza");
        assert_eq!(read[1].phone, None);

        let poll = Extra::Poll(Poll {
            question: "Pizza?".into(),
            options: vec!["Yes".into(), "No".into()],
            multiple: false,
        });
        assert_eq!(decode(&encode(&poll)), Some(poll));
    }

    #[test]
    fn keeps_buttons_as_they_were() {
        let buttons = Extra::Buttons(Buttons {
            footer: Some("Atendimento".into()),
            buttons: vec![
                Button {
                    label: "Sim".into(),
                    action: ButtonAction::Reply(Answer::Template {
                        id: "yes".into(),
                        index: 0,
                    }),
                },
                Button {
                    label: "Site".into(),
                    action: ButtonAction::Url("https://example.com".into()),
                },
                Button {
                    label: "Ligar".into(),
                    action: ButtonAction::Call("+5511999999999".into()),
                },
            ],
        });
        assert_eq!(decode(&encode(&buttons)), Some(buttons));
        let bare = Extra::Buttons(Buttons {
            footer: None,
            buttons: vec![Button {
                label: "Opções".into(),
                action: ButtonAction::List,
            }],
        });
        assert_eq!(decode(&encode(&bare)), Some(bare));
    }
}
