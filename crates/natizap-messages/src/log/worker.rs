//! The message store's worker: off the UI thread, it files every change and
//! rebuilds the open chat's rows when they are affected.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::Local;
use natizap_protocol::whatsapp_rust::prelude::Event;
use natizap_protocol::{ChatId, Unread};

use super::{Input, OpenChat, Output, Pinned, SHOWN_MESSAGES};
use crate::items::{items, summary};
use crate::media::{fetch_on_sight, posters_on_sight};
use crate::message::MessageChange;
use crate::store::MessageStore;
use crate::sync::changes;

/// Stores incoming changes and rebuilds the open chat's rows when it is
/// affected, off the UI thread.
pub(super) async fn sync(
    store_path: PathBuf,
    inputs: async_channel::Receiver<Input>,
    outputs: async_channel::Sender<Output>,
) {
    let Some(mut store) = open_store(&store_path) else {
        return;
    };
    let mut open: Option<OpenChat> = None;
    let mut community_admins: Vec<ChatId> = Vec::new();

    while let Ok(input) = inputs.recv().await {
        let mut refresh = false;
        let mut opened = false;
        let mut arrived: Vec<Unread> = Vec::new();
        let mut next = Some(input);
        while let Some(input) = next.take() {
            match input {
                Input::Open(chat) => {
                    open = Some(chat);
                    community_admins.clear();
                    opened = true;
                }
                Input::CommunityAdmins(chat, admins) => {
                    if open.as_ref().is_some_and(|open| open.id == chat) {
                        community_admins = admins;
                        refresh = true;
                    }
                }
                Input::Event(event) if matches!(*event, Event::LoggedOut(_)) => {
                    // Whoever links next may be a different account.
                    log_error(store.clear(), "could not clear the message store");
                    refresh = true;
                }
                Input::Event(event) => {
                    let changes = changes(&event);
                    refresh |= touches(open.as_ref(), &changes);
                    arrived.extend(arrivals(open.as_ref(), &changes));
                    log_error(store.apply(&changes), "could not store messages");
                }
                Input::Apply(change) => {
                    refresh |= touches(open.as_ref(), std::slice::from_ref(&change));
                    log_error(store.apply(&[*change]), "could not store a message");
                }
            }
            next = inputs.try_recv().ok();
        }

        let Some(chat) = open.as_ref() else { continue };
        if opened && chat.unread > 0 {
            match store.latest_incoming(&chat.addresses, chat.unread) {
                Ok(messages) => arrived.extend(messages),
                Err(error) => tracing::error!(%error, "could not read unread messages"),
            }
        }
        if !arrived.is_empty() {
            let output = Output::Unread {
                chat: chat.id.clone(),
                messages: arrived,
            };
            if outputs.send(output).await.is_err() {
                return;
            }
        }
        if opened || refresh {
            let thread = thread(&store, chat, &community_admins);
            if outputs.send(thread).await.is_err() {
                return;
            }
        }
    }
}

/// The open chat's rows and what they need fetched, read from `store`.
fn thread(store: &MessageStore, chat: &OpenChat, community_admins: &[ChatId]) -> Output {
    let messages = store
        .latest(&chat.addresses, SHOWN_MESSAGES)
        .unwrap_or_else(|error| {
            tracing::error!(%error, "could not read messages");
            Vec::new()
        });
    let reactions = store.reactions(&messages).unwrap_or_else(|error| {
        tracing::error!(%error, "could not read reactions");
        HashMap::new()
    });
    let rows = items(
        &messages,
        &reactions,
        &chat.title,
        community_admins,
        Local::now(),
    )
    .into();
    let fetch = fetch_on_sight(&messages);
    let posters = posters_on_sight(&messages);
    let pinned = store
        .pinned(&chat.addresses, Local::now().timestamp())
        .unwrap_or_else(|error| {
            tracing::error!(%error, "could not read the pinned message");
            None
        })
        .map(|message| Pinned {
            id: message.id.clone().into(),
            text: summary(&message).into(),
        });
    Output::Thread {
        chat: chat.id.clone(),
        items: rows,
        fetch,
        posters,
        pinned,
    }
}

/// Messages others sent to the chat on screen: read as they arrive.
fn arrivals<'a>(
    open: Option<&'a OpenChat>,
    changes: &'a [MessageChange],
) -> impl Iterator<Item = Unread> + 'a {
    changes.iter().filter_map(move |change| match change {
        MessageChange::Received(message)
            if !message.from_me && open?.addresses.contains(&message.chat) =>
        {
            Some(Unread {
                id: message.id.clone(),
                sender: message.sender.clone(),
            })
        }
        _ => None,
    })
}

fn touches(open: Option<&OpenChat>, changes: &[MessageChange]) -> bool {
    open.is_some_and(|open| {
        changes.iter().any(|change| {
            change
                .chat()
                .is_none_or(|chat| open.addresses.contains(chat))
        })
    })
}

fn open_store(path: &Path) -> Option<MessageStore> {
    MessageStore::open(path)
        .or_else(|error| {
            // Conversations still work for this session without the file.
            tracing::error!(%error, path = %path.display(), "could not open the message store");
            MessageStore::in_memory()
        })
        .inspect_err(|error| tracing::error!(%error, "could not create an in-memory store"))
        .ok()
}

fn log_error(result: rusqlite::Result<()>, context: &str) {
    if let Err(error) = result {
        tracing::error!(%error, "{context}");
    }
}
