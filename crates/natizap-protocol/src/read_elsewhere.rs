//! What this account read on its other devices, such as the phone.
//!
//! The client turns read receipts into events that drop the `recipient`
//! attribute, which is the only place a receipt from one of the account's
//! own devices names the chat it read. So these are read from the raw
//! stanzas instead.

use async_channel::Receiver;
use gpui::{App, AppContext as _};
use whatsapp_rust::Jid;
use whatsapp_rust::prelude::{Event, EventKind};
use whatsapp_rust::wacore_binary::{JidExt as _, NodeRef};

use crate::{ChatId, Protocol};

/// Messages this account read or played on another of its devices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadElsewhere {
    /// The chat they are in; `status@broadcast` for status updates.
    pub chat: ChatId,
    /// The messages read, the newest first as WhatsApp lists them.
    pub ids: Vec<String>,
}

impl Protocol {
    /// What this account reads on its other devices from now on, and what
    /// it read while natizap was away: the server replays those receipts on
    /// connecting.
    pub fn reads_elsewhere(&self, cx: &App) -> Receiver<ReadElsewhere> {
        let nodes = self.subscribe(&[EventKind::RawNode]);
        let connection = self.connection.clone();
        let (tx, reads) = async_channel::unbounded();
        cx.background_spawn(async move {
            while let Ok(event) = nodes.recv().await {
                let Event::RawNode(node) = &*event else {
                    continue;
                };
                let Ok(client) = connection.client() else {
                    continue;
                };
                let own: Vec<Jid> = [client.pn(), client.lid()].into_iter().flatten().collect();
                let Some(read) = Receipt::of(node.get()).and_then(|receipt| receipt.by(&own))
                else {
                    continue;
                };
                if tx.send(read).await.is_err() {
                    break;
                }
            }
        })
        .detach();
        reads
    }
}

/// The parts of a read receipt stanza that say who read what.
#[derive(Debug)]
struct Receipt {
    from: Jid,
    participant: Option<Jid>,
    recipient: Option<Jid>,
    ids: Vec<String>,
}

impl Receipt {
    /// `node` as a read receipt, if it is one.
    fn of(node: &NodeRef<'_>) -> Option<Self> {
        if node.tag != "receipt" {
            return None;
        }
        let mut attrs = node.attrs();
        let kind = attrs.optional_string("type")?;
        if !matches!(&*kind, "read" | "read-self" | "played" | "played-self") {
            return None;
        }
        let first = attrs.optional_string("id")?.into_owned();
        let more = node
            .get_optional_child("list")
            .into_iter()
            .flat_map(|list| list.get_children_by_tag("item"))
            .filter_map(|item| {
                item.attrs()
                    .optional_string("id")
                    .map(std::borrow::Cow::into_owned)
            });
        Some(Self {
            from: attrs.optional_jid("from")?,
            participant: attrs.optional_jid("participant"),
            recipient: attrs.optional_jid("recipient"),
            ids: std::iter::once(first).chain(more).collect(),
        })
    }

    /// What this receipt says the account read, when one of its devices,
    /// going by any of the `own` addresses, sent it; receipts from others
    /// reading the account's messages say nothing of the kind.
    fn by(self, own: &[Jid]) -> Option<ReadElsewhere> {
        let mine = |jid: &Jid| {
            own.iter()
                .any(|own| own.user == jid.user && own.server == jid.server)
        };
        let chat = if mine(&self.from) {
            // From the account itself: the chat is the recipient, or the
            // chat with itself.
            self.recipient.unwrap_or_else(|| self.from.to_non_ad())
        } else if (self.from.is_group() || self.from.is_status_broadcast())
            && self.participant.as_ref().is_some_and(mine)
        {
            self.from
        } else {
            return None;
        };
        Some(ReadElsewhere {
            chat: ChatId::new(&chat.to_non_ad().to_string()),
            ids: self.ids,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[expect(clippy::expect_used, reason = "test addresses are written valid")]
    fn jid(text: &str) -> Jid {
        text.parse().expect("a valid test address")
    }

    fn receipt(from: &str, participant: Option<&str>, recipient: Option<&str>) -> Receipt {
        Receipt {
            from: jid(from),
            participant: participant.map(jid),
            recipient: recipient.map(jid),
            ids: vec!["M1".into()],
        }
    }

    fn own() -> Vec<Jid> {
        vec![
            jid("5511900000000@s.whatsapp.net"),
            jid("100000000000001@lid"),
        ]
    }

    fn chat_of(receipt: Receipt) -> Option<String> {
        receipt.by(&own()).map(|read| read.chat.as_str().to_owned())
    }

    #[test]
    fn a_chat_read_on_the_phone_names_its_recipient() {
        let read = receipt("100000000000001:0@lid", None, Some("200000000000002@lid"));
        assert_eq!(chat_of(read).as_deref(), Some("200000000000002@lid"));
    }

    #[test]
    fn a_group_or_status_read_on_the_phone_is_the_group_or_status() {
        let group = receipt(
            "120363000000000000@g.us",
            Some("5511900000000:0@s.whatsapp.net"),
            None,
        );
        assert_eq!(chat_of(group).as_deref(), Some("120363000000000000@g.us"));
        let status = receipt("status@broadcast", Some("100000000000001:0@lid"), None);
        assert_eq!(chat_of(status).as_deref(), Some("status@broadcast"));
    }

    #[test]
    fn someone_else_reading_is_not_the_account_reading() {
        assert_eq!(chat_of(receipt("200000000000002@lid", None, None)), None);
        let group = receipt("120363000000000000@g.us", Some("200000000000002@lid"), None);
        assert_eq!(chat_of(group), None);
        let viewer = receipt("status@broadcast", Some("200000000000002@lid"), None);
        assert_eq!(chat_of(viewer), None);
    }
}
