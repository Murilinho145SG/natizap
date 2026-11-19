//! Profile pictures of people and groups.

use std::sync::Arc;

use whatsapp_rust::Jid;
use whatsapp_rust::http::HttpRequest;
use whatsapp_rust::prelude::Client;
use whatsapp_rust::wacore::iq::usync::UserInfo;

use crate::ActionError;
use crate::business::{self, Business};

/// What a person's or business's profile shows besides their picture.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Profile {
    /// Their about line, when they share it with this account.
    pub about: Option<String>,
    /// Set for business accounts.
    pub business: Option<Business>,
}

pub(crate) async fn profile(client: Arc<Client>, jid: Jid) -> Result<Profile, ActionError> {
    let rejected = |error: &dyn std::fmt::Display| ActionError::refused(error);
    let Some(info) = user_info(&client, &jid).await? else {
        // Not on WhatsApp, or it would not say: nothing to show.
        return Ok(Profile::default());
    };
    let business = if info.is_business {
        let name = info.verified_name.and_then(|verified| verified.name);
        Some(
            business::business(&client, &jid, name)
                .await
                .map_err(|error| rejected(&error))?,
        )
    } else {
        None
    };
    Ok(Profile {
        about: about_of(info.status),
        business,
    })
}

/// The about line of `jid`, if it shares one: the profile without the
/// business query behind it.
pub(crate) async fn about(client: Arc<Client>, jid: Jid) -> Result<Option<String>, ActionError> {
    Ok(user_info(&client, &jid)
        .await?
        .and_then(|info| about_of(info.status)))
}

/// What WhatsApp's contacts service says of `jid`, if anything. It limits
/// how often it may be asked.
async fn user_info(client: &Client, jid: &Jid) -> Result<Option<UserInfo>, ActionError> {
    Ok(client
        .contacts()
        .get_user_info(std::slice::from_ref(jid))
        .await
        .map_err(|error| ActionError::refused(&error))?
        .remove(jid))
}

fn about_of(status: Option<String>) -> Option<String> {
    status.filter(|about| !about.trim().is_empty())
}

/// The profile picture of `jid`, small or `full` size, or `None` when it has
/// none or hides it from this account.
pub(crate) async fn picture(
    client: Arc<Client>,
    jid: Jid,
    full: bool,
) -> Result<Option<Vec<u8>>, ActionError> {
    let rejected = |error: &dyn std::fmt::Display| ActionError::refused(error);
    // The preview is 96 pixels square, plenty for avatars and a fraction of
    // the full picture's size.
    let Some(picture) = client
        .contacts()
        .get_profile_picture(&jid, !full)
        .await
        .map_err(|error| rejected(&error))?
    else {
        return Ok(None);
    };
    let response = client
        .http_client
        .execute(HttpRequest::get(picture.url))
        .await
        .map_err(|error| rejected(&error))?;
    if !(200..300).contains(&response.status_code) {
        return Err(ActionError::Rejected(format!(
            "the picture server answered {}",
            response.status_code
        )));
    }
    Ok(Some(response.body))
}
