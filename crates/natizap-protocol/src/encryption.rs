//! A chat's security code: the 60 digits two people compare, each on
//! their own phone or computer, to know their messages and calls reach
//! only each other. WhatsApp shows the same code for the same keys.

use gpui::{App, Task};
use whatsapp_rust::SecurityCodeVersion;

use crate::Protocol;
use crate::actions::{self, ActionError};
use crate::chat::ChatId;

/// The 60 digits of a chat's security code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecurityCode(String);

impl SecurityCode {
    /// `digits`, if they are a whole code: 60 ASCII digits.
    fn new(digits: String) -> Option<Self> {
        (digits.len() == 60 && digits.bytes().all(|byte| byte.is_ascii_digit()))
            .then_some(Self(digits))
    }

    /// The code as WhatsApp shows it: twelve groups of five digits.
    pub fn groups(&self) -> impl Iterator<Item = &str> {
        // Whole ASCII digits: every five bytes are a boundary.
        (0..12).map(|group| &self.0[group * 5..group * 5 + 5])
    }
}

impl Protocol {
    /// The security code of the chat with `peer`, as it is now: worked out
    /// from every device's keys on both sides, which it fetches first.
    /// It changes when either side links, removes or reinstalls a device.
    ///
    /// # Errors
    ///
    /// Fails offline, for groups, and when a key or device list cannot be
    /// had whole: a partial code would not match the other side's.
    pub fn security_code(
        &self,
        peer: &ChatId,
        cx: &App,
    ) -> Task<Result<SecurityCode, ActionError>> {
        let jid = match actions::jid(peer) {
            Ok(jid) => jid,
            Err(error) => return Task::ready(Err(error)),
        };
        self.connection.run(cx, move |client| async move {
            // WhatsApp's phones show the code by phone number.
            let digits = client
                .signal()
                .security_code(&jid, SecurityCodeVersion::V3)
                .await
                .map_err(|error| ActionError::refused(&error))?;
            SecurityCode::new(digits)
                .ok_or_else(|| ActionError::refused(&"the security code was not 60 digits"))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::SecurityCode;

    #[test]
    fn a_code_shows_as_twelve_groups_of_five() {
        let digits: String = (0..60).map(|index| char::from(b'0' + index % 10)).collect();
        let code = SecurityCode::new(digits)
            .map(|code| code.groups().map(str::to_owned).collect::<Vec<_>>());
        let groups = code.unwrap_or_default();
        assert_eq!(groups.len(), 12);
        assert_eq!(groups[0], "01234");
        assert_eq!(groups[11], "56789");
    }

    #[test]
    fn only_sixty_digits_make_a_code() {
        assert!(SecurityCode::new("1".repeat(59)).is_none());
        assert!(SecurityCode::new(format!("{}x", "1".repeat(59))).is_none());
    }
}
