use std::sync::Arc;

use phonenumber::Mode;

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

    /// The phone number behind a person's JID, when the JID carries one,
    /// written the way its country writes it (`+55 11 98765-4321`);
    /// privacy-preserving LID addresses carry none.
    #[must_use]
    pub fn phone_number(&self) -> Option<String> {
        let user = self.0.strip_suffix("@s.whatsapp.net")?;
        let number = format!("+{user}");
        let Ok(parsed) = phonenumber::parse(None, &number) else {
            return Some(number);
        };
        let national = parsed.national().value().to_string();
        Some(if phonenumber::is_valid(&parsed) {
            parsed.format().mode(Mode::International).to_string()
        } else if let Some(old_mobile) = old_brazilian_mobile(parsed.code().value(), &national) {
            old_mobile
        } else {
            number
        })
    }

    /// Whether this is a conversation with people. Status updates and
    /// channels are not: they have their own screens.
    #[must_use]
    pub fn is_conversation(&self) -> bool {
        !(self.0.ends_with("@broadcast") || self.0.ends_with("@newsletter"))
    }
}

/// WhatsApp keeps many Brazilian mobiles in their form from before the
/// extra 9 (`+55 41 9900-0198`), which libphonenumber no longer accepts or
/// formats; the phone still writes them this way.
fn old_brazilian_mobile(country: u16, national: &str) -> Option<String> {
    let mobile = country == 55
        && national.len() == 10
        && national
            .as_bytes()
            .get(2)
            .is_some_and(|digit| (b'6'..=b'9').contains(digit));
    mobile.then(|| {
        format!(
            "+55 {} {}-{}",
            &national[..2],
            &national[2..6],
            &national[6..]
        )
    })
}

/// What someone is called: the name saved in the phone's address book,
/// shown as is, and the one they chose for themselves, shown after a `~`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PersonName {
    /// Saved in the address book.
    pub saved: Option<String>,
    /// Chosen by the person.
    pub chosen: Option<String>,
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

    #[test]
    fn writes_phone_numbers_the_way_their_country_does() {
        let phone = |jid: &str| ChatId::new(jid).phone_number();
        assert_eq!(
            phone("5511987654321@s.whatsapp.net").as_deref(),
            Some("+55 11 98765-4321")
        );
        assert_eq!(
            phone("554133221100@s.whatsapp.net").as_deref(),
            Some("+55 41 3322-1100")
        );
        assert_eq!(
            phone("14155552671@s.whatsapp.net").as_deref(),
            Some("+1 415-555-2671")
        );
        assert_eq!(
            phone("554199000198@s.whatsapp.net").as_deref(),
            Some("+55 41 9900-0198")
        );
        assert_eq!(phone("5511999@s.whatsapp.net").as_deref(), Some("+5511999"));
        assert_eq!(phone("211862499156117@lid"), None);
    }
}
