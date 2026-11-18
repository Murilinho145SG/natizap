//! The rail on the window's left edge: the chats, the settings, and your
//! own avatar, which opens your profile in place of the chat list.

use gpui::prelude::*;
use gpui::{Context, px};
use natizap_contacts::{OwnProfile, OwnProfileClosed};
use natizap_notifications::{NotificationSettings, SettingsClosed};
use natizap_ui::{Button, ButtonVariant, Icon, RAIL_AVATAR, Rail, avatar, rail_avatar};

use super::Root;

impl Root {
    /// The rail, Chats lit unless your profile shows.
    pub(super) fn rail(&mut self, cx: &mut Context<Self>) -> Rail {
        let me = self.protocol.read(cx).me();
        let on_profile = self.own_shown;
        let mut rail = Rail::new()
            .top(
                Button::new("rail-chats")
                    .icon(Icon::Chats)
                    .variant(ButtonVariant::Ghost)
                    .round()
                    .selected(!on_profile)
                    .tooltip("Chats")
                    .on_click(cx.listener(|root, _, _, cx| {
                        root.close_own_profile(cx);
                    })),
            )
            .bottom(
                Button::new("rail-settings")
                    .icon(Icon::Settings)
                    .variant(ButtonVariant::Ghost)
                    .round()
                    .selected(self.settings.is_some())
                    .tooltip("Settings")
                    .on_click(cx.listener(|root, _, _, cx| root.open_settings(cx))),
            );
        if let Some(me) = me {
            let picture = self
                .pictures
                .update(cx, |pictures, cx| pictures.picture(&me.phone, cx));
            let name = me.name.clone().unwrap_or_default();
            rail = rail.bottom(
                rail_avatar(
                    "rail-me",
                    avatar(me.phone.as_str(), &name, picture, px(RAIL_AVATAR)),
                    on_profile,
                )
                .tooltip(natizap_ui::Tooltip::text("Profile"))
                .on_click(cx.listener(|root, _, _, cx| root.toggle_own_profile(cx))),
            );
        }
        rail
    }

    /// Shows the notification settings over the chats.
    fn open_settings(&mut self, cx: &mut Context<Self>) {
        let notifier = self.notifier.clone();
        let settings = cx.new(|cx| NotificationSettings::new(notifier, cx));
        cx.subscribe(&settings, |root, _, _: &SettingsClosed, cx| {
            root.settings = None;
            cx.notify();
        })
        .detach();
        self.settings = Some(settings);
        cx.notify();
    }

    /// Opens your profile in place of the chat list, or goes back to the
    /// chats if it shows.
    fn toggle_own_profile(&mut self, cx: &mut Context<Self>) {
        if self.own_shown {
            self.close_own_profile(cx);
            return;
        }
        if let Some(profile) = &self.own_profile {
            profile.update(cx, OwnProfile::reload_missing);
            self.own_shown = true;
            cx.notify();
            return;
        }
        let Some(me) = self.protocol.read(cx).me() else {
            return;
        };
        let (protocol, pictures) = (self.protocol.clone(), self.pictures.clone());
        let profile = cx.new(|cx| OwnProfile::new(&protocol, &pictures, me, cx));
        cx.subscribe(&profile, |root, _, _: &OwnProfileClosed, cx| {
            root.close_own_profile(cx);
        })
        .detach();
        self.own_profile = Some(profile);
        self.own_shown = true;
        cx.notify();
    }

    /// Goes back from your profile to the chats. Returns whether it showed.
    pub(super) fn close_own_profile(&mut self, cx: &mut Context<Self>) -> bool {
        let shown = std::mem::take(&mut self.own_shown);
        if shown {
            cx.notify();
        }
        shown
    }
}
