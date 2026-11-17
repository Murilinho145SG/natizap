use std::sync::Arc;

/// A chat's address: the WhatsApp JID of the person, group or broadcast list,
/// without a device suffix.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChatId(Arc<str>);

impl ChatId {
    /// Normalizes a JID string: `5511999:3@s.whatsapp.net` and
    /// `5511999@s.whatsapp.net` address the same chat.
    #[must_use]
    pub fn new(jid: &str) -> Self {
        match jid.split_once('@') {
            Some((user, server)) => {
                let user = user.split_once(':').map_or(user, |(user, _device)| user);
                Self(format!("{user}@{server}").into())
            }
            None => Self(jid.into()),
        }
    }

    /// The JID as a string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether this is a group chat.
    #[must_use]
    pub fn is_group(&self) -> bool {
        self.0.ends_with("@g.us")
    }

    /// The phone number behind a person's JID, when the JID carries one;
    /// privacy-preserving LID addresses do not.
    #[must_use]
    pub fn phone_number(&self) -> Option<String> {
        let user = self.0.strip_suffix("@s.whatsapp.net")?;
        Some(format!("+{user}"))
    }

    /// Whether this is a conversation with people. Status updates and
    /// channels are not: they have their own screens.
    #[must_use]
    pub fn is_conversation(&self) -> bool {
        !(self.0.ends_with("@broadcast") || self.0.ends_with("@newsletter"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_suffix_does_not_split_a_chat() {
        assert_eq!(
            ChatId::new("5511999:3@s.whatsapp.net"),
            ChatId::new("5511999@s.whatsapp.net")
        );
    }
}
