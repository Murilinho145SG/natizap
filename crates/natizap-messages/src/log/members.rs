//! The open group's members, named as the header lists them and as the
//! composer offers them to mention.

use gpui::App;
use natizap_protocol::ChatId;

use super::MessageLog;

/// A member of the open group who can be mentioned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Mentionable {
    /// Their address in the group, which the mention names.
    pub(crate) address: ChatId,
    /// The address book's name, else the one they chose after a `~`, else
    /// their number.
    pub(crate) name: String,
    /// Their number, beside a name that is not it.
    pub(crate) phone: Option<String>,
}

impl MessageLog {
    /// The open group's members as its header lists them: names saved in
    /// the address book, then names people chose after a `~`, then
    /// numbers, each in order, and this account last.
    pub(crate) fn members_line(&self, cx: &App) -> Option<String> {
        let (members, own) = self.members(cx)?;
        let mut line: Vec<String> = members.into_iter().map(|member| member.name).collect();
        if own {
            line.push("You".to_owned());
        }
        Some(line.join(", "))
    }

    /// Whom the open group lets this account mention: everyone else in
    /// it, in the header's order. Empty outside groups.
    pub(crate) fn mentionable(&self, cx: &App) -> Vec<Mentionable> {
        self.members(cx)
            .map_or_else(Vec::new, |(members, _)| members)
    }

    /// The open group's other members, in order, and whether this account
    /// is among them.
    fn members(&self, cx: &App) -> Option<(Vec<Mentionable>, bool)> {
        let group = self.group.as_ref()?;
        let protocol = self.protocol.read(cx);
        let mut own = false;
        let mut listed: Vec<(u8, Mentionable)> = Vec::new();
        for member in &group.members {
            let addresses = [Some(&member.address), member.phone.as_ref()];
            let addresses = || addresses.iter().flatten();
            if addresses().any(|address| protocol.is_own(address)) {
                own = true;
                continue;
            }
            let known = addresses().find_map(|address| self.names.get(*address));
            let phone = addresses().find_map(|address| address.phone_number());
            let (rank, name, phone) = match (
                known.and_then(|known| known.saved.clone()),
                known.and_then(|known| known.chosen.clone()),
            ) {
                (Some(saved), _) => (0, saved, phone),
                (None, Some(chosen)) => (1, format!("~ {chosen}"), phone),
                (None, None) => (
                    2,
                    phone
                        .clone()
                        .unwrap_or_else(|| member.address.as_str().to_owned()),
                    None,
                ),
            };
            listed.push((
                rank,
                Mentionable {
                    address: member.address.clone(),
                    name,
                    phone,
                },
            ));
        }
        listed.sort_by(|a, b| {
            a.0.cmp(&b.0)
                .then_with(|| a.1.name.to_lowercase().cmp(&b.1.name.to_lowercase()))
        });
        Some((listed.into_iter().map(|(_, member)| member).collect(), own))
    }
}
