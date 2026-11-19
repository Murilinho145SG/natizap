//! A poll's standing: how many chose each option, what this account chose,
//! and what voting on it takes.

use std::collections::HashMap;
use std::sync::Arc;

use gpui::SharedString;
use natizap_protocol::{ChatId, Extra};

use crate::message::{Message, Reaction};
use crate::store::Vote;

/// What has been said about the messages on screen besides themselves.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Addons<'a> {
    /// Reactions, by message id.
    pub(crate) reactions: &'a HashMap<String, Vec<Reaction>>,
    /// Votes, by poll id.
    pub(crate) votes: &'a HashMap<String, Vec<Vote>>,
}

/// Someone who voted on a poll, as the poll shows them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Voter {
    /// Their address, for their picture; `None` for this account.
    pub(crate) id: Option<ChatId>,
    pub(crate) name: SharedString,
}

/// A poll as its bubble shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PollState {
    /// Who chose each option, in the poll's order.
    pub(crate) chosen_by: Vec<Vec<Voter>>,
    /// The options this account chose.
    pub(crate) mine: Vec<String>,
    /// How many people voted.
    pub(crate) voters: usize,
    /// What a vote needs; `None` for a poll natizap cannot vote on, one it
    /// saw before it kept the secret votes are sealed with.
    pub(crate) ballot: Option<Ballot>,
}

/// What casting a vote on a poll takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Ballot {
    pub(crate) chat: ChatId,
    pub(crate) poll_id: String,
    /// Who made the poll; `None` for this account.
    pub(crate) creator: Option<ChatId>,
    pub(crate) secret: Arc<[u8]>,
}

impl PollState {
    /// The standing of `message`, if it is a poll, given its `votes`;
    /// `name` says what each voter is called.
    pub(crate) fn of(
        message: &Message,
        votes: &[Vote],
        name: impl Fn(&ChatId) -> SharedString,
    ) -> Option<Self> {
        let Some(Extra::Poll(poll)) = &message.extra else {
            return None;
        };
        let chosen_by = poll
            .options
            .iter()
            .map(|option| {
                votes
                    .iter()
                    .filter(|vote| vote.options.contains(option))
                    .map(|vote| Voter {
                        id: vote.voter.clone(),
                        name: vote.voter.as_ref().map_or_else(|| "You".into(), &name),
                    })
                    .collect()
            })
            .collect();
        let mine = votes
            .iter()
            .find(|vote| vote.mine())
            .map_or_else(Vec::new, |vote| vote.options.clone());
        let ballot = message.secret.as_deref().map(|secret| Ballot {
            chat: message.chat.clone(),
            poll_id: message.id.clone(),
            creator: (!message.from_me).then(|| {
                message
                    .sender
                    .clone()
                    .unwrap_or_else(|| message.chat.clone())
            }),
            secret: secret.into(),
        });
        Some(Self {
            chosen_by,
            mine,
            voters: votes.len(),
            ballot,
        })
    }

    /// How many chose each option, in the poll's order.
    pub(crate) fn counts(&self) -> impl Iterator<Item = usize> + '_ {
        self.chosen_by.iter().map(Vec::len)
    }

    /// What this account chooses after clicking `option`: on a poll with one
    /// answer the click moves the choice there, or takes it back when it
    /// was already there; with several answers it toggles that one.
    pub(crate) fn after_click(&self, option: &str, multiple: bool, all: &[String]) -> Vec<String> {
        let chosen = self.mine.iter().any(|mine| mine == option);
        if !multiple {
            return if chosen {
                Vec::new()
            } else {
                vec![option.to_owned()]
            };
        }
        // Kept in the poll's order, as WhatsApp sends them.
        all.iter()
            .filter(|each| {
                if each.as_str() == option {
                    !chosen
                } else {
                    self.mine.contains(each)
                }
            })
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(mine: &[&str]) -> PollState {
        PollState {
            chosen_by: Vec::new(),
            mine: mine.iter().map(|&option| option.to_owned()).collect(),
            voters: 0,
            ballot: None,
        }
    }

    fn options() -> Vec<String> {
        vec!["A".into(), "B".into(), "C".into()]
    }

    #[test]
    fn one_answer_moves_or_is_taken_back() {
        assert_eq!(state(&[]).after_click("B", false, &options()), ["B"]);
        assert_eq!(state(&["A"]).after_click("B", false, &options()), ["B"]);
        assert_eq!(
            state(&["B"]).after_click("B", false, &options()),
            Vec::<String>::new()
        );
    }

    #[test]
    fn several_answers_toggle_in_the_polls_order() {
        assert_eq!(state(&["C"]).after_click("A", true, &options()), ["A", "C"]);
        assert_eq!(state(&["A", "C"]).after_click("A", true, &options()), ["C"]);
    }
}
