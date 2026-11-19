//! What a conversation needs to know about a group: who runs it, and who
//! may write.

use std::sync::Arc;

use whatsapp_rust::Jid;
use whatsapp_rust::prelude::Client;
use whatsapp_rust::wacore::iq::groups::ParticipantType;

use crate::actions;
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
    /// Each member's phone-number address by their other address, so a
    /// member writing under a LID still shows a number.
    pub phones: Vec<(ChatId, ChatId)>,
    /// The group's name.
    pub subject: String,
    /// What the group says about itself.
    pub description: Option<String>,
    /// Everyone in the group.
    pub members: Vec<Member>,
    /// Who created it, by phone number when known.
    pub creator: Option<ChatId>,
    /// When it was created, in Unix seconds.
    pub created_at: Option<i64>,
    /// The community it belongs to.
    pub community: Option<ChatId>,
}

/// Someone in a group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    /// Their address in the group.
    pub address: ChatId,
    /// Their phone-number address, when the group shares it.
    pub phone: Option<ChatId>,
    /// What they may do there.
    pub role: Role,
}

/// What a member may do in a group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    /// Created the group: an admin who cannot be removed.
    Owner,
    /// Runs the group.
    Admin,
    /// Writes and reads.
    Member,
}

pub(crate) async fn info(client: Arc<Client>, group: Jid) -> Result<GroupInfo, ActionError> {
    let groups = client.groups();
    let mut metadata = groups
        .fetch_metadata(&group)
        .await
        .map_err(|error| ActionError::Rejected(error.to_string()))?;
    // Members' phone numbers, which the server leaves out in groups that go
    // by LID.
    groups.resolve_participant_addresses(&mut metadata).await;
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
    let phones = metadata
        .participants
        .iter()
        .filter_map(|participant| {
            let phone = ChatId::new(&participant.phone_number.as_ref()?.to_string());
            Some(
                [Some(&participant.jid), participant.lid.as_ref()]
                    .into_iter()
                    .flatten()
                    .map(move |address| (ChatId::new(&address.to_string()), phone.clone())),
            )
        })
        .flatten()
        .collect();
    let own = actions::own_addresses(&client);
    let address = |jid: &Jid| ChatId::new(&jid.to_string());
    let members = metadata
        .participants
        .iter()
        .map(|participant| Member {
            address: address(&participant.jid),
            phone: participant.phone_number.as_ref().map(address),
            role: match participant.participant_type {
                ParticipantType::SuperAdmin => Role::Owner,
                ParticipantType::Admin => Role::Admin,
                ParticipantType::Member => Role::Member,
            },
        })
        .collect();
    Ok(GroupInfo {
        own_admin: own.iter().any(|own| admins.contains(own)),
        admins,
        admins_only: metadata.is_announcement,
        community_announcements: metadata.is_default_sub_group,
        phones,
        subject: metadata.subject.unwrap_or_default(),
        description: metadata
            .description
            .filter(|description| !description.trim().is_empty()),
        members,
        creator: metadata
            .creator_pn
            .as_ref()
            .or(metadata.creator.as_ref())
            .map(address),
        created_at: metadata
            .creation_time
            .and_then(|seconds| i64::try_from(seconds).ok()),
        community: metadata.parent_group_jid.as_ref().map(address),
    })
}
