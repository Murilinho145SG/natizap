//! Finding messages by their text, in one chat or in every chat, through
//! the trigram index (`messages_fts`).

use natizap_protocol::ChatId;
use rusqlite::params_from_iter;
use rusqlite::types::Value;

use super::{MessageStore, message_from_row};
use crate::message::Message;
use crate::text_search::Case;

/// The fewest characters the trigram index can look up; shorter queries
/// read the chat's messages one by one.
const TRIGRAM: usize = 3;

/// Every way of writing `query` with each letter small or capital.
fn spellings(query: &str) -> Vec<String> {
    query.chars().fold(vec![String::new()], |spelled, ch| {
        let lower: String = ch.to_lowercase().collect();
        let upper: String = ch.to_uppercase().collect();
        let forms = if lower == upper {
            vec![lower]
        } else {
            vec![lower, upper]
        };
        spelled
            .iter()
            .flat_map(|start| forms.iter().map(move |form| format!("{start}{form}")))
            .collect()
    })
}

impl MessageStore {
    /// The messages whose text holds `query`, newest first, at most
    /// `limit`: in `chats` when given, else in every chat but status
    /// updates.
    pub(crate) fn search(
        &self,
        chats: Option<&[ChatId]>,
        query: &str,
        case: Case,
        limit: usize,
    ) -> rusqlite::Result<Vec<Message>> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let mut values: Vec<Value> = Vec::new();
        let (source, matching) = if query.chars().count() >= TRIGRAM {
            // A quoted FTS5 string: the query as one run of characters.
            values.push(Value::from(format!("\"{}\"", query.replace('"', "\"\""))));
            (
                "messages_fts JOIN messages ON messages.rowid = messages_fts.rowid",
                "messages_fts MATCH ?".to_owned(),
            )
        } else {
            // Too short for the index. SQLite folds ASCII letters only, so
            // every way of writing the query's one or two characters is
            // looked for, at most four.
            let spellings = match case {
                Case::Sensitive => vec![query.to_owned()],
                Case::Insensitive => spellings(query),
            };
            let tests = vec!["instr(messages.text, ?) > 0"; spellings.len()].join(" OR ");
            values.extend(spellings.into_iter().map(Value::from));
            ("messages", format!("({tests})"))
        };
        let scope = match chats {
            Some(chats) => {
                values.extend(
                    chats
                        .iter()
                        .map(|chat| Value::from(chat.as_str().to_owned())),
                );
                format!("messages.chat IN ({})", vec!["?"; chats.len()].join(", "))
            }
            None => "messages.chat NOT LIKE '%@broadcast'".to_owned(),
        };
        // A case-sensitive search drops some of what the index finds, so
        // it asks for more to fill `limit`.
        let asked = match case {
            Case::Sensitive => limit.saturating_mul(4),
            Case::Insensitive => limit,
        };
        values.push(Value::from(i64::try_from(asked).unwrap_or(i64::MAX)));
        let found: Vec<Message> = self
            .connection
            .prepare(&format!(
                "SELECT messages.* FROM {source}
                 WHERE {matching} AND {scope} AND messages.deleted = 0
                 ORDER BY messages.sent_at DESC LIMIT ?"
            ))?
            .query_map(params_from_iter(values), message_from_row)?
            .collect::<rusqlite::Result<_>>()?;
        Ok(found
            .into_iter()
            .filter(|message| {
                message
                    .text
                    .as_deref()
                    .is_some_and(|text| case.finds(text, query))
            })
            .take(limit)
            .collect())
    }
}
