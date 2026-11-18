//! Votes on polls: kept one per voter, the latest replacing the one
//! before, and opened from their sealed form with the poll's secret.

use std::collections::HashMap;

use natizap_protocol::{ChatId, Extra, SealedVote};
use rusqlite::{OptionalExtension as _, Transaction, params, params_from_iter};

use super::MessageStore;
use super::rows::message_from_row;
use crate::message::{Message, MessageChange};

/// One person's choices on a poll.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Vote {
    pub(crate) options: Vec<String>,
    /// Whether this account cast it.
    pub(crate) mine: bool,
}

/// Keeps `voter`'s vote on `poll_id` (`None` is this account), unless the
/// one kept was cast later. No options take the vote back.
pub(super) fn record(
    tx: &Transaction<'_>,
    poll_id: &str,
    voter: Option<&ChatId>,
    options: &[String],
    cast_at: i64,
) -> rusqlite::Result<()> {
    let voter = voter.map_or("", ChatId::as_str);
    // Options are single lines, so a newline separates them.
    let options = options.join("\n");
    tx.prepare_cached(
        "INSERT INTO votes (poll_id, voter, options, cast_at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (poll_id, voter) DO UPDATE SET
             options = excluded.options, cast_at = excluded.cast_at
         WHERE excluded.cast_at >= votes.cast_at",
    )?
    .execute(params![poll_id, voter, options, cast_at])?;
    Ok(())
}

impl MessageStore {
    /// The votes on each poll among `messages`, by poll id. Votes taken
    /// back are left out.
    pub(crate) fn votes(
        &self,
        messages: &[Message],
    ) -> rusqlite::Result<HashMap<String, Vec<Vote>>> {
        let polls: Vec<&str> = messages
            .iter()
            .filter(|message| matches!(message.extra, Some(Extra::Poll(_))))
            .map(|message| message.id.as_str())
            .collect();
        let mut votes: HashMap<String, Vec<Vote>> = HashMap::new();
        if polls.is_empty() {
            return Ok(votes);
        }
        let placeholders = vec!["?"; polls.len()].join(", ");
        let mut statement = self.connection.prepare(&format!(
            "SELECT poll_id, voter, options FROM votes
             WHERE poll_id IN ({placeholders}) AND options != ''"
        ))?;
        let rows = statement.query_map(params_from_iter(polls), |row| {
            let cast_by: String = row.get(1)?;
            let options: String = row.get(2)?;
            Ok((
                row.get::<_, String>(0)?,
                Vote {
                    options: options.lines().map(str::to_owned).collect(),
                    mine: cast_by.is_empty(),
                },
            ))
        })?;
        for row in rows {
            let (poll, vote) = row?;
            votes.entry(poll).or_default().push(vote);
        }
        Ok(votes)
    }

    /// Opens each vote among `changes` that arrived sealed, with the secret
    /// of its poll; `own` are this account's addresses, which key the
    /// votes on its own polls. A vote that cannot be opened, on a poll from
    /// before natizap kept their secrets, is dropped.
    pub(crate) fn open_votes(
        &self,
        changes: Vec<MessageChange>,
        own: impl Fn() -> Vec<ChatId>,
    ) -> Vec<MessageChange> {
        changes
            .into_iter()
            .filter_map(|change| match change {
                MessageChange::VoteArrived(vote) => self.open_vote(&vote, &own),
                change => Some(change),
            })
            .collect()
    }

    fn open_vote(
        &self,
        vote: &SealedVote,
        own: &impl Fn() -> Vec<ChatId>,
    ) -> Option<MessageChange> {
        let poll = self
            .connection
            .query_row(
                "SELECT * FROM messages WHERE id = ?1 AND secret IS NOT NULL",
                [&vote.poll_id],
                message_from_row,
            )
            .optional()
            .inspect_err(|error| tracing::error!(%error, "could not read a poll"))
            .ok()
            .flatten();
        let Some(poll) = poll else {
            tracing::debug!(poll = vote.poll_id, "vote on an unknown poll");
            return None;
        };
        let (Some(Extra::Poll(question)), Some(secret)) = (&poll.extra, &poll.secret) else {
            return None;
        };
        let creators = if poll.from_me {
            own()
        } else {
            // In a one-to-one chat the other side made it.
            std::iter::once(poll.sender.clone().unwrap_or_else(|| poll.chat.clone()))
                .chain(poll.sender_phone.clone())
                .collect()
        };
        let Some(options) = vote.open(question, secret, &creators) else {
            tracing::debug!(poll = vote.poll_id, "could not open a vote");
            return None;
        };
        Some(MessageChange::Voted {
            poll_id: vote.poll_id.clone(),
            voter: vote.voter.clone(),
            options,
            cast_at: vote.cast_at,
        })
    }
}
