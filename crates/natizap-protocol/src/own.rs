//! This account's own profile: who it is, and changing its name, about
//! line and picture on all its devices.

use gpui::{App, Task};

use crate::business::{self, Business};
use crate::profile;
use crate::{ActionError, ChatId, Protocol};

/// This account, as its profile shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Me {
    /// The name people see, once one was set.
    pub name: Option<String>,
    /// Its phone-number address.
    pub phone: ChatId,
    /// Its LID, once WhatsApp gave it one.
    pub lid: Option<ChatId>,
}

impl Protocol {
    /// This account, once the device is linked.
    #[must_use]
    pub fn me(&self) -> Option<Me> {
        let client = self.connection.client().ok()?;
        let address = |jid: whatsapp_rust::Jid| ChatId::new(&jid.to_non_ad().to_string());
        let name = client.push_name();
        Some(Me {
            name: Some(name).filter(|name| !name.trim().is_empty()),
            phone: address(client.pn()?),
            lid: client.lid().map(address),
        })
    }

    /// This account's about line, if it has one.
    pub fn own_about(&self, cx: &App) -> Task<Result<Option<String>, ActionError>> {
        self.connection.run(cx, move |client| async move {
            let jid = client.pn().ok_or(ActionError::Offline)?.to_non_ad();
            profile::about(client, jid).await
        })
    }

    /// This account's business details, or `None` when it is not a
    /// business account.
    pub fn own_business(&self, cx: &App) -> Task<Result<Option<Business>, ActionError>> {
        self.connection.run(cx, move |client| async move {
            let jid = client.pn().ok_or(ActionError::Offline)?.to_non_ad();
            let name = Some(client.push_name()).filter(|name| !name.trim().is_empty());
            business::business_if_any(&client, &jid, name)
                .await
                .map_err(|error| ActionError::Rejected(error.to_string()))
        })
    }

    /// Renames this account: the name people see, on all its devices.
    pub fn set_own_name(&self, name: String, cx: &App) -> Task<Result<(), ActionError>> {
        self.connection.run(cx, move |client| async move {
            client
                .profile()
                .set_push_name(&name)
                .await
                .map_err(|error| ActionError::Rejected(error.to_string()))
        })
    }

    /// Changes this account's about line.
    pub fn set_own_about(&self, about: String, cx: &App) -> Task<Result<(), ActionError>> {
        self.connection.run(cx, move |client| async move {
            client
                .profile()
                .set_status_text(&about)
                .await
                .map_err(|error| ActionError::Rejected(error.to_string()))
        })
    }

    /// Makes `jpeg`, already cropped square, this account's picture.
    pub fn set_own_picture(&self, jpeg: Vec<u8>, cx: &App) -> Task<Result<(), ActionError>> {
        self.connection.run(cx, move |client| async move {
            client
                .profile()
                .set_profile_picture(jpeg)
                .await
                .map(|_| ())
                .map_err(|error| ActionError::Rejected(error.to_string()))
        })
    }

    /// Removes this account's picture.
    pub fn remove_own_picture(&self, cx: &App) -> Task<Result<(), ActionError>> {
        self.connection.run(cx, move |client| async move {
            client
                .profile()
                .remove_profile_picture()
                .await
                .map(|_| ())
                .map_err(|error| ActionError::Rejected(error.to_string()))
        })
    }
}
