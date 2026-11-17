//! How a message's shared contacts or poll are kept in the `extra` column:
//! one line per entry, fields split by tabs.

use natizap_protocol::{Card, Extra, Poll};

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
}
