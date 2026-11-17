//! A group's profile: its members, who runs it, and where it came from.

use std::cmp::Ordering;
use std::collections::HashMap;

use chrono::{Local, TimeZone as _};
use gpui::prelude::*;
use gpui::{Context, Div, FontWeight, SharedString, Stateful, div, px, rgb};
use natizap_protocol::{ChatId, GroupInfo, Member, PersonName, Role};
use natizap_ui::theme::{MUTED, RAISED, TEXT};
use natizap_ui::{BadgeTone, avatar, badge, separator};

use crate::panel::ProfilePanel;

/// The user picked someone in a group to see their profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonOpened {
    /// Their address.
    pub chat: ChatId,
    /// Their name as listed.
    pub title: SharedString,
}

/// Members listed before "View all".
const FIRST_MEMBERS: usize = 10;
const MEMBER_PICTURE: f32 = 40.0;

/// How a member is listed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Listed {
    pub(crate) address: ChatId,
    /// Where their profile is asked for: their phone-number address when
    /// known, as a LID tells less.
    pub(crate) profile: ChatId,
    /// Their saved name, "~ chosen", "You", or their number.
    pub(crate) name: String,
    /// Their number, beside a name that is not saved.
    pub(crate) phone: Option<String>,
    pub(crate) role: Role,
    pub(crate) own: bool,
}

/// `members` as the phone lists them: this account first, then admins,
/// then names saved in the address book, then the rest, each by name.
pub(crate) fn listed(
    members: &[Member],
    names: &HashMap<ChatId, PersonName>,
    is_own: impl Fn(&ChatId) -> bool,
) -> Vec<Listed> {
    let mut listed: Vec<(bool, Listed)> = members
        .iter()
        .map(|member| {
            let own = is_own(&member.address) || member.phone.as_ref().is_some_and(&is_own);
            let known = names
                .get(&member.address)
                .or_else(|| member.phone.as_ref().and_then(|phone| names.get(phone)));
            let phone = member
                .phone
                .as_ref()
                .or(Some(&member.address))
                .and_then(ChatId::phone_number);
            let saved = known.and_then(|known| known.saved.clone());
            let (name, phone) = if own {
                ("You".to_owned(), None)
            } else if let Some(saved) = &saved {
                (saved.clone(), None)
            } else if let Some(chosen) = known.and_then(|known| known.chosen.as_ref()) {
                (format!("~ {chosen}"), phone)
            } else {
                (
                    phone
                        .clone()
                        .unwrap_or_else(|| member.address.as_str().to_owned()),
                    None,
                )
            };
            let entry = Listed {
                address: member.address.clone(),
                profile: member
                    .phone
                    .clone()
                    .unwrap_or_else(|| member.address.clone()),
                name,
                phone,
                role: member.role,
                own,
            };
            (saved.is_some(), entry)
        })
        .collect();
    listed.sort_by(|(a_saved, a), (b_saved, b)| {
        let admin = |listed: &Listed| listed.role != Role::Member;
        b.own
            .cmp(&a.own)
            .then(admin(b).cmp(&admin(a)))
            .then(b_saved.cmp(a_saved))
            .then_with(|| compare_names(&a.name, &b.name))
    });
    listed.into_iter().map(|(_, entry)| entry).collect()
}

fn compare_names(a: &str, b: &str) -> Ordering {
    a.trim_start_matches("~ ")
        .to_lowercase()
        .cmp(&b.trim_start_matches("~ ").to_lowercase())
}

impl ProfilePanel {
    /// The group's member count heading, its members and how it began.
    pub(crate) fn members(&self, group: &GroupInfo, cx: &mut Context<Self>) -> Div {
        let listed = listed(&group.members, &self.names, |address| {
            self.protocol.read(cx).is_own(address)
        });
        let total = listed.len();
        let shown = if self.all_members {
            total
        } else {
            total.min(FIRST_MEMBERS)
        };
        let rows: Vec<Stateful<Div>> = listed
            .iter()
            .take(shown)
            .enumerate()
            .map(|(index, member)| {
                let picture = self
                    .pictures
                    .update(cx, |pictures, cx| pictures.picture(&member.address, cx));
                let row = member_row(member, picture).id(("member", index));
                if member.own {
                    return row;
                }
                let opened = PersonOpened {
                    chat: member.profile.clone(),
                    title: member.name.trim_start_matches("~ ").to_owned().into(),
                };
                row.cursor_pointer()
                    .hover(|row| row.bg(rgb(RAISED)))
                    .on_click(cx.listener(move |_, _, _, cx| cx.emit(opened.clone())))
            })
            .collect();
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap_1()
            .child(div().w_full().py_3().child(separator()))
            .child(
                div()
                    .pb_2()
                    .text_size(px(13.5))
                    .text_color(rgb(MUTED))
                    .child(format!("{total} members")),
            )
            .children(rows)
            .when(shown < total, |list| {
                list.child(
                    div()
                        .id("all-members")
                        .px_2()
                        .py_2()
                        .rounded_lg()
                        .cursor_pointer()
                        .hover(|row| row.bg(rgb(RAISED)))
                        .text_size(px(14.5))
                        .child(format!("View all ({} more)", total - shown))
                        .on_click(cx.listener(|panel, _, _, cx| {
                            panel.all_members = true;
                            cx.notify();
                        })),
                )
            })
            .children(origin(group, &self.names).map(|origin| {
                div()
                    .pt_4()
                    .text_size(px(12.5))
                    .text_color(rgb(MUTED))
                    .child(origin)
            }))
    }
}

fn member_row(member: &Listed, picture: Option<std::path::PathBuf>) -> Div {
    div()
        .px_2()
        .py_1p5()
        .flex()
        .items_center()
        .gap_3()
        .rounded_lg()
        .child(avatar(
            member.address.as_str(),
            member.name.trim_start_matches("~ "),
            picture,
            px(MEMBER_PICTURE),
        ))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(14.5))
                .text_color(rgb(TEXT))
                .child(member.name.clone()),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .items_end()
                .gap_0p5()
                .when(member.role != Role::Member, |side| {
                    side.child(badge(SharedString::from("Group admin"), BadgeTone::Muted))
                })
                .children(member.phone.clone().map(|phone| {
                    div()
                        .text_size(px(12.0))
                        .text_color(rgb(MUTED))
                        .child(phone)
                })),
        )
        .font_weight(FontWeight::NORMAL)
}

/// "Created by Ana on 05/02/2025 at 06:27", as far as the group says.
fn origin(group: &GroupInfo, names: &HashMap<ChatId, PersonName>) -> Option<String> {
    let created = group
        .created_at
        .and_then(|seconds| Local.timestamp_opt(seconds, 0).single())
        .map(|at| at.format("on %d/%m/%Y at %H:%M").to_string());
    let creator = group.creator.as_ref().map(|creator| {
        let known = names.get(creator);
        known
            .and_then(|known| known.saved.clone())
            .or_else(|| {
                known.and_then(|known| known.chosen.as_ref().map(|name| format!("~ {name}")))
            })
            .or_else(|| creator.phone_number())
            .unwrap_or_else(|| creator.as_str().to_owned())
    });
    match (creator, created) {
        (Some(creator), Some(created)) => Some(format!("Created by {creator} {created}")),
        (Some(creator), None) => Some(format!("Created by {creator}")),
        (None, Some(created)) => Some(format!("Created {created}")),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(address: &str, phone: Option<&str>, role: Role) -> Member {
        Member {
            address: ChatId::new(address),
            phone: phone.map(ChatId::new),
            role,
        }
    }

    #[test]
    fn lists_you_then_admins_then_saved_names() {
        let members = [
            member("1@lid", Some("5511911111111@s.whatsapp.net"), Role::Member),
            member("2@lid", Some("5511922222222@s.whatsapp.net"), Role::Member),
            member("3@lid", None, Role::Admin),
            member("4@lid", None, Role::Member),
        ];
        let names = HashMap::from([
            (
                ChatId::new("5511922222222@s.whatsapp.net"),
                PersonName {
                    saved: Some("Bia".into()),
                    chosen: None,
                },
            ),
            (
                ChatId::new("1@lid"),
                PersonName {
                    saved: None,
                    chosen: Some("Caio".into()),
                },
            ),
        ]);
        let listed = listed(&members, &names, |address| address.as_str() == "4@lid");
        let shown: Vec<(&str, Option<&str>)> = listed
            .iter()
            .map(|member| (member.name.as_str(), member.phone.as_deref()))
            .collect();
        assert_eq!(
            shown,
            [
                ("You", None),
                ("3@lid", None),
                ("Bia", None),
                ("~ Caio", Some("+55 11 91111-1111")),
            ]
        );
    }
}
