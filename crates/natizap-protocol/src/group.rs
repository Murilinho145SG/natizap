//! What a conversation needs to know about a group: who runs it, and who
//! may write.

use std::sync::Arc;

use whatsapp_rust::Jid;
use whatsapp_rust::prelude::Client;
use whatsapp_rust::wacore::iq::groups::ParticipantType;

use crate::{ActionError, ChatId};

/// A group as its conversation shows it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupInfo {
    /// Every address of every admin: a person may write under their LID or
    /// their phone number.
    pub admins: Vec<ChatId>,
    /// Only admins may send messages.
    pub admins_only: bool,
    /// The announcements group of a community, where community admins post
    /// to every member.
    pub community_announcements: bool,
    /// This account is among the admins.
    pub own_admin: bool,
}

pub(crate) async fn info(client: Arc<Client>, group: Jid) -> Result<GroupInfo, ActionError> {
    let metadata = client
        .groups()
        .get_metadata(&group)
        .await
        .map_err(|error| ActionError::Rejected(error.to_string()))?;
    let admins: Vec<ChatId> = metadata
        .participants
        .iter()
        .filter(|participant| {
            matches!(
                participant.participant_type,
                ParticipantType::Admin | ParticipantType::SuperAdmin
            )
        })
        .flat_map(|participant| {
            [
                Some(&participant.jid),
                participant.phone_number.as_ref(),
                participant.lid.as_ref(),
            ]
        })
        .flatten()
        .map(|jid| ChatId::new(&jid.to_string()))
        .collect();
    let own: Vec<ChatId> = [client.pn(), client.lid()]
        .into_iter()
        .flatten()
        .map(|jid| ChatId::new(&jid.to_string()))
        .collect();
    Ok(GroupInfo {
        own_admin: own.iter().any(|own| admins.contains(own)),
        admins,
        admins_only: metadata.is_announcement,
        community_announcements: metadata.is_default_sub_group,
    })
}
