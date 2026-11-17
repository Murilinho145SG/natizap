//! Profile pictures of people and groups.

use std::sync::Arc;

use whatsapp_rust::Jid;
use whatsapp_rust::http::HttpRequest;
use whatsapp_rust::prelude::Client;

use crate::ActionError;

/// What a person's or business's profile shows besides their picture.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Profile {
    /// Their about line, when they share it with this account.
    pub about: Option<String>,
    /// Set for business accounts.
    pub business: Option<Business>,
}

/// A business account's public details.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Business {
    /// The name WhatsApp verified for it, shown instead of a number.
    pub name: Option<String>,
    /// What kind of business it is, e.g. "Restaurant".
    pub category: Option<String>,
    /// How it describes itself.
    pub description: Option<String>,
    /// Where it is.
    pub address: Option<String>,
    /// How to write to it.
    pub email: Option<String>,
    /// Its websites.
    pub websites: Vec<String>,
}

pub(crate) async fn profile(client: Arc<Client>, jid: Jid) -> Result<Profile, ActionError> {
    let rejected = |error: &dyn std::fmt::Display| ActionError::Rejected(error.to_string());
    let Some(info) = client
        .contacts()
        .get_user_info(std::slice::from_ref(&jid))
        .await
        .map_err(|error| rejected(&error))?
        .remove(&jid)
    else {
        // Not on WhatsApp, or it would not say: nothing to show.
        return Ok(Profile::default());
    };
    let business = if info.is_business {
        let details = client
            .get_business_profile(&jid)
            .await
            .map_err(|error| rejected(&error))?;
        let non_empty = |text: String| Some(text).filter(|text| !text.trim().is_empty());
        Some(Business {
            name: info.verified_name.and_then(|verified| verified.name),
            category: details
                .as_ref()
                .and_then(|details| details.categories.first())
                .map(|category| category.name.clone()),
            description: details
                .as_ref()
                .and_then(|details| non_empty(details.description.clone())),
            address: details.as_ref().and_then(|details| details.address.clone()),
            email: details.as_ref().and_then(|details| details.email.clone()),
            websites: details.map_or_default(|details| details.website),
        })
    } else {
        None
    };
    Ok(Profile {
        about: info.status.filter(|about| !about.trim().is_empty()),
        business,
    })
}

/// The profile picture of `jid`, small or `full` size, or `None` when it has
/// none or hides it from this account.
pub(crate) async fn picture(
    client: Arc<Client>,
    jid: Jid,
    full: bool,
) -> Result<Option<Vec<u8>>, ActionError> {
    let rejected = |error: &dyn std::fmt::Display| ActionError::Rejected(error.to_string());
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
