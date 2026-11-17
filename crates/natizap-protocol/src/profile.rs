//! Profile pictures of people and groups.

use std::sync::Arc;

use whatsapp_rust::Jid;
use whatsapp_rust::http::HttpRequest;
use whatsapp_rust::prelude::Client;

use crate::ActionError;

/// The small profile picture of `jid`, or `None` when it has none or hides
/// it from this account.
pub(crate) async fn picture(client: Arc<Client>, jid: Jid) -> Result<Option<Vec<u8>>, ActionError> {
    let rejected = |error: &dyn std::fmt::Display| ActionError::Rejected(error.to_string());
    // The preview is 96 pixels square, plenty for avatars and a fraction of
    // the full picture's size.
    let Some(picture) = client
        .contacts()
        .get_profile_picture(&jid, true)
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
