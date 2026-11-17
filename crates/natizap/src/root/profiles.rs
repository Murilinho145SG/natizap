//! Profiles beside the conversation, and the chats they lead to.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::{Context, Entity, SharedString};
use natizap_contacts::{
    ChatSettings, ChatSummary, ChatWanted, DisappearingChosen, MuteToggled, PersonOpened,
    ProfileClosed, ProfilePanel, Tile,
};
use natizap_messages::{ConversationView, OpenChat, ProfileRequested};
use natizap_protocol::ChatId;

use super::Root;

impl Root {
    /// Shows a profile beside the conversation when the conversation asks:
    /// the chat's from its header, or a group member's from their message.
    pub(super) fn open_profiles(conversation: &Entity<ConversationView>, cx: &mut Context<Self>) {
        cx.subscribe(conversation, |root, _, request: &ProfileRequested, cx| {
            root.show_profile(&request.chat, request.title.clone(), request.verified, cx);
        })
        .detach();
    }

    /// Shows `chat`'s profile beside the conversation, named `title` until
    /// it loads.
    fn show_profile(
        &mut self,
        chat: &ChatId,
        title: SharedString,
        verified: bool,
        cx: &mut Context<Self>,
    ) {
        let (protocol, pictures) = (self.protocol.clone(), self.pictures.clone());
        let panel =
            cx.new(|cx| ProfilePanel::new(&protocol, &pictures, chat.clone(), title, verified, cx));
        cx.subscribe(&panel, |root, _, _: &ProfileClosed, cx| {
            root.profile = None;
            cx.notify();
        })
        .detach();
        cx.subscribe(&panel, |root, _, opened: &PersonOpened, cx| {
            root.show_profile(&opened.chat, opened.title.clone(), false, cx);
        })
        .detach();
        cx.subscribe(&panel, |root, _, wanted: &ChatWanted, cx| {
            root.chat_with(&wanted.chat, wanted.title.clone(), cx);
        })
        .detach();
        let names = Arc::clone(self.chats.read(cx).names());
        panel.update(cx, |panel, cx| panel.set_names(names, cx));
        self.fill_profile(&panel, chat, cx);
        self.profile = Some(panel);
        cx.notify();
    }

    /// Opens the chat with `address`, even one not in the list yet, named
    /// `title` until it has messages.
    pub(super) fn chat_with(
        &mut self,
        address: &ChatId,
        title: SharedString,
        cx: &mut Context<Self>,
    ) {
        let row = self
            .chats
            .read(cx)
            .rows()
            .iter()
            .chain(self.chats.read(cx).archived().iter())
            .find(|row| row.addresses.contains(address))
            .cloned();
        self.history.visit(address.clone());
        if let Some(row) = row {
            self.open_chat(&row, cx);
        } else {
            self.profile = None;
            let chat = OpenChat {
                id: address.clone(),
                verified: false,
                addresses: vec![address.clone()],
                title,
                unread: 0,
                muted: false,
            };
            self.messages
                .update(cx, |messages, cx| messages.open(chat, cx));
            self.chat_list.update(cx, |list, cx| list.show(None, cx));
        }
        cx.notify();
    }

    /// Gives the profile of `chat` what the other features know of it: its
    /// media and starred messages, and its notifications and disappearing
    /// messages, which it can change.
    fn fill_profile(&self, panel: &Entity<ProfilePanel>, chat: &ChatId, cx: &mut Context<Self>) {
        let Some(row) = self
            .chats
            .read(cx)
            .rows()
            .iter()
            .find(|row| row.addresses.contains(chat))
            .cloned()
        else {
            return;
        };
        let settings = ChatSettings {
            muted: row.muted,
            disappearing: row.disappearing,
        };
        panel.update(cx, |panel, cx| panel.set_settings(settings, cx));

        let reading = self.messages.read(cx).overview(row.addresses.to_vec(), cx);
        let summarized = panel.downgrade();
        cx.spawn(async move |_, cx| {
            let overview = reading.await;
            let summary = ChatSummary {
                media: overview.media,
                tiles: overview
                    .recent
                    .into_iter()
                    .map(|tile| Tile {
                        image: tile.image.map(Into::into),
                        length: tile.length,
                    })
                    .collect(),
                starred: overview.starred,
            };
            let _ = summarized.update(cx, |panel, cx| panel.set_summary(summary, cx));
        })
        .detach();

        let (chats, id) = (self.chats.clone(), row.id.clone());
        cx.subscribe(panel, move |_, _, MuteToggled(muted), cx| {
            chats.read(cx).set_muted(&id, *muted, cx);
        })
        .detach();
        let (chats, id) = (self.chats.clone(), row.id.clone());
        cx.subscribe(panel, move |_, _, DisappearingChosen(timer), cx| {
            chats.read(cx).set_disappearing(&id, *timer, cx);
        })
        .detach();
    }
}
