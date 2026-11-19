//! Forgetting messages the user deleted from their devices: one, or a
//! whole chat's when they clear or delete it.

use natizap_protocol::ChatId;
use rusqlite::{Transaction, params};

/// Forgets message `id` in `chat`, with its reactions and votes.
pub(super) fn remove(tx: &Transaction<'_>, chat: &ChatId, id: &str) -> rusqlite::Result<()> {
    tx.prepare_cached("DELETE FROM messages WHERE chat = ?1 AND id = ?2")?
        .execute(params![chat.as_str(), id])?;
    tx.prepare_cached("DELETE FROM reactions WHERE message_id = ?1")?
        .execute(params![id])?;
    tx.prepare_cached("DELETE FROM votes WHERE poll_id = ?1")?
        .execute(params![id])?;
    Ok(())
}

/// Forgets the messages filed under `chat`, with their reactions and
/// votes; starred ones stay when `keep_starred`.
pub(super) fn forget(
    tx: &Transaction<'_>,
    chat: &ChatId,
    keep_starred: bool,
) -> rusqlite::Result<()> {
    let gone = "SELECT id FROM messages WHERE chat = ?1 AND (?2 = 0 OR starred = 0)";
    tx.prepare_cached(&format!(
        "DELETE FROM reactions WHERE message_id IN ({gone})"
    ))?
    .execute(params![chat.as_str(), keep_starred])?;
    tx.prepare_cached(&format!("DELETE FROM votes WHERE poll_id IN ({gone})"))?
        .execute(params![chat.as_str(), keep_starred])?;
    tx.prepare_cached("DELETE FROM messages WHERE chat = ?1 AND (?2 = 0 OR starred = 0)")?
        .execute(params![chat.as_str(), keep_starred])?;
    Ok(())
}
