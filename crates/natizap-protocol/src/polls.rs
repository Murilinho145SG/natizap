//! Votes on polls. A vote travels sealed with the poll's secret, keyed to
//! the addresses of the poll's creator and of the voter, and names the
//! options it chose by their hashes.

use gpui::{App, Task};
use whatsapp_rust::Jid;
use whatsapp_rust::prelude::{MessageExt as _, wa};
use whatsapp_rust::types::events::InboundMessage;
use whatsapp_rust::wacore::poll::{self, PollVoteCiphertext};

use crate::actions::{self, ActionError, Connection, Sent};
use crate::{ChatId, Poll, Protocol};

impl Poll {
    /// The options among `hashes`, the way a vote names what it chose, in
    /// the poll's order.
    #[must_use]
    pub fn chosen<H: AsRef<[u8]>>(&self, hashes: &[H]) -> Vec<String> {
        self.options
            .iter()
            .filter(|option| {
                let hash = poll::compute_option_hash(option);
                hashes.iter().any(|chosen| chosen.as_ref() == hash)
            })
            .cloned()
            .collect()
    }
}

/// A vote on a poll as it arrives: still sealed with the poll's secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedVote {
    /// WhatsApp's id of the poll voted on.
    pub poll_id: String,
    /// Who voted; `None` for this account, from any of its devices.
    pub voter: Option<ChatId>,
    /// When it was cast, in Unix milliseconds: a later vote replaces an
    /// earlier one.
    pub cast_at: i64,
    /// The voter's addresses, either of which may key the vote.
    addresses: Vec<String>,
    payload: Vec<u8>,
    iv: Vec<u8>,
}

impl SealedVote {
    /// The vote `inbound` carries, if it is one.
    #[must_use]
    pub fn of(inbound: &InboundMessage) -> Option<Self> {
        let update = inbound
            .message
            .get_base_message()
            .poll_update_message
            .as_option()?;
        let vote = update.vote.as_option()?;
        let source = &inbound.info.source;
        Some(Self {
            poll_id: update.poll_creation_message_key.as_option()?.id.clone()?,
            voter: (!source.is_from_me)
                .then(|| ChatId::new(&source.sender.to_non_ad().to_string())),
            cast_at: update
                .sender_timestamp_ms
                .unwrap_or_else(|| inbound.info.timestamp.as_millis()),
            addresses: std::iter::once(&source.sender)
                .chain(&source.sender_alt)
                .map(|jid| jid.to_non_ad().to_string())
                .collect(),
            payload: vote.enc_payload.clone()?,
            iv: vote.enc_iv.clone()?,
        })
    }

    /// The options the vote chose on `poll`, opened with the poll's
    /// `secret`; `creators` are the addresses of whoever made the poll. An
    /// empty list takes a previous vote back. `None` when no address opens
    /// it.
    #[must_use]
    pub fn open(&self, poll: &Poll, secret: &[u8], creators: &[ChatId]) -> Option<Vec<String>> {
        let creators: Vec<String> = creators.iter().filter_map(non_ad).collect();
        // A vote is keyed to the namespace (phone number or LID) each side
        // was known by when it was cast, which a sealed vote does not say:
        // every pairing is tried, and the cipher's tag rejects wrong ones.
        let hashes = creators.iter().find_map(|creator| {
            self.addresses.iter().find_map(|voter| {
                poll::decrypt_poll_vote_with_secret(
                    PollVoteCiphertext {
                        enc_payload: &self.payload,
                        enc_iv: &self.iv,
                    },
                    secret,
                    &self.poll_id,
                    creator,
                    voter,
                )
                .ok()
            })
        })?;
        Some(poll.chosen(&hashes))
    }
}

/// `address` without its device, as votes are keyed.
fn non_ad(address: &ChatId) -> Option<String> {
    let jid: Jid = address.as_str().parse().ok()?;
    Some(jid.to_non_ad().to_string())
}

/// The secret `message` carries, which keys the votes on a poll.
pub(crate) fn secret(message: &wa::Message) -> Option<Vec<u8>> {
    message
        .message_context_info
        .as_option()?
        .message_secret
        .clone()
}

impl Protocol {
    /// Votes for `options` on poll `poll_id` in `chat`, replacing this
    /// account's previous vote; no options take it back. `creator` made the
    /// poll, `None` for this account; `secret` is the poll's.
    pub fn vote(
        &self,
        chat: &ChatId,
        poll_id: String,
        creator: Option<&ChatId>,
        secret: Vec<u8>,
        options: Vec<String>,
        cx: &App,
    ) -> Task<Result<Sent, ActionError>> {
        let chat = match actions::jid(chat) {
            Ok(chat) => chat,
            Err(error) => return Task::ready(Err(error)),
        };
        let creator = match creator.map(actions::jid).transpose() {
            Ok(creator) => creator,
            Err(error) => return Task::ready(Err(error)),
        };
        self.connection.run(cx, move |client| async move {
            // The client tells its own polls by the phone number.
            let creator = match creator {
                Some(creator) => creator,
                None => client.pn().ok_or(ActionError::Offline)?.to_non_ad(),
            };
            let sent = client
                .polls()
                .vote_raw(chat, &poll_id, &creator, &secret, &options)
                .await
                .map_err(|error| ActionError::Rejected(error.to_string()))?;
            Ok(Sent {
                id: sent.message_id.into_string(),
                secret: None,
            })
        })
    }

    /// This account's addresses, readable from any thread.
    #[must_use]
    pub fn account(&self) -> Account {
        Account(self.connection.clone())
    }
}

/// This account, as polls need it: whoever made a poll keys its votes.
#[derive(Debug, Clone)]
pub struct Account(Connection);

impl Account {
    /// Its phone number and LID, once the device is linked.
    #[must_use]
    pub fn addresses(&self) -> Vec<ChatId> {
        self.0
            .client()
            .map_or_else(|_| Vec::new(), |client| actions::own_addresses(&client))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_vote_names_options_by_their_hashes() {
        let poll = Poll {
            question: "Lunch?".into(),
            options: vec!["Pizza".into(), "Sushi".into(), "Tacos".into()],
            multiple: true,
        };
        let hashes = [
            poll::compute_option_hash("Tacos"),
            poll::compute_option_hash("Pizza"),
            poll::compute_option_hash("Not an option"),
        ];
        assert_eq!(poll.chosen(&hashes), ["Pizza", "Tacos"]);
    }

    #[test]
    fn a_vote_opens_with_the_addresses_it_was_sealed_for() {
        let poll = Poll {
            question: "Lunch?".into(),
            options: vec!["Pizza".into(), "Sushi".into()],
            multiple: false,
        };
        let secret = [7u8; 32];
        let creator = "5511900000001@s.whatsapp.net";
        let voter = "5511900000002@s.whatsapp.net";
        let (payload, iv) = poll::encrypt_poll_vote_with_secret(
            &[poll::compute_option_hash("Sushi").to_vec()],
            &secret,
            "POLL1",
            creator,
            voter,
        )
        .unwrap_or_default();
        let vote = SealedVote {
            poll_id: "POLL1".into(),
            voter: Some(ChatId::new(voter)),
            cast_at: 0,
            addresses: vec!["123@lid".into(), voter.into()],
            payload,
            iv: iv.to_vec(),
        };
        let creators = [ChatId::new("456@lid"), ChatId::new(creator)];
        assert_eq!(
            vote.open(&poll, &secret, &creators),
            Some(vec!["Sushi".to_owned()])
        );
        assert_eq!(vote.open(&poll, &[8u8; 32], &creators), None);
    }
}
