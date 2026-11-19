//! The account's call history, kept in step with WhatsApp in the
//! background.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use gpui::{AppContext as _, Context, Entity, SharedString, Task};
use natizap_protocol::whatsapp_rust::prelude::Event;
use natizap_protocol::{CallFollower, CallRecord, ChatId, Protocol};

use crate::group::{CallGroup, collapse};
use crate::store::CallStore;

/// What the history's worker hears.
enum Input {
    Event(Arc<Event>),
    /// Calls kept elsewhere before the history had them.
    Import(Vec<CallRecord>),
}

/// Every call that rang on the account's devices or was made from its
/// phone, newest first.
pub struct CallLog {
    calls: Vec<CallRecord>,
    inputs: async_channel::Sender<Input>,
    /// What people and groups are called, by every address they go by.
    titles: HashMap<ChatId, SharedString>,
    _tasks: [Task<()>; 3],
}

/// Calls the history just filed, from any of its sources, for their chats
/// to show.
#[derive(Debug, Clone)]
pub struct CallsFiled(pub Vec<CallRecord>);

impl gpui::EventEmitter<CallsFiled> for CallLog {}

impl std::fmt::Debug for CallLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CallLog")
            .field("calls", &self.calls.len())
            .finish_non_exhaustive()
    }
}

impl CallLog {
    /// Keeps the call history in the store at `path`, learning calls
    /// through `protocol`.
    pub fn new(protocol: &Entity<Protocol>, path: PathBuf, cx: &mut Context<Self>) -> Self {
        let events = protocol.read(cx).subscribe(CallFollower::EVENTS);
        let mut follower = protocol.read(cx).call_follower();
        let (inputs_tx, inputs) = async_channel::unbounded();
        let forward_tx = inputs_tx.clone();
        let forward = cx.background_spawn(async move {
            while let Ok(event) = events.recv().await {
                if forward_tx.send(Input::Event(event)).await.is_err() {
                    break;
                }
            }
        });
        let (lists_tx, lists) = async_channel::unbounded();
        let work = cx.background_spawn(async move {
            let opened = CallStore::open(&path).or_else(|error| {
                tracing::error!(%error, "could not open the call history; keeping it in memory");
                CallStore::in_memory()
            });
            let mut store = match opened {
                Ok(store) => store,
                Err(error) => {
                    tracing::error!(%error, "no call history at all");
                    return;
                }
            };
            // At first, every call kept: each shows in its chat, those
            // kept before chats showed calls among them. A call already
            // in its chat stays as it was.
            let mut filed = None;
            loop {
                match store.recent() {
                    Ok(calls) => {
                        let filed = filed.take().unwrap_or_else(|| calls.clone());
                        if lists_tx.send((calls, filed)).await.is_err() {
                            return;
                        }
                    }
                    Err(error) => tracing::error!(%error, "could not read the call history"),
                }
                // Waits for the next call that ended, or calls to import.
                let (kept, calls) = loop {
                    let Ok(input) = inputs.recv().await else {
                        return;
                    };
                    match input {
                        Input::Event(event) => {
                            let ended = follower.hear(&event);
                            if !ended.is_empty() {
                                break (store.keep(&ended), ended);
                            }
                        }
                        Input::Import(calls) => break (store.keep_new(&calls), calls),
                    }
                };
                filed = Some(match kept {
                    Ok(()) => calls,
                    Err(error) => {
                        tracing::error!(%error, "could not keep calls");
                        Vec::new()
                    }
                });
            }
        });
        let apply = cx.spawn(async move |this, cx| {
            while let Ok((calls, filed)) = lists.recv().await {
                if this
                    .update(cx, |log, cx| {
                        log.calls = calls;
                        if !filed.is_empty() {
                            cx.emit(CallsFiled(filed));
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            calls: Vec::new(),
            inputs: inputs_tx,
            titles: HashMap::new(),
            _tasks: [forward, work, apply],
        }
    }

    /// Takes `calls` kept elsewhere before the history had them, such as the
    /// missed calls the message store kept; calls already here stay as
    /// they are.
    pub fn import(&self, calls: Vec<CallRecord>) {
        if self.inputs.try_send(Input::Import(calls)).is_err() {
            tracing::error!("call history worker stopped");
        }
    }

    /// The calls as rows, newest first, back-to-back ones together.
    #[must_use]
    pub fn groups(&self) -> Vec<CallGroup> {
        collapse(&self.calls)
    }

    /// What `chat` is called.
    #[must_use]
    pub fn title(&self, chat: &ChatId) -> SharedString {
        self.titles.get(chat).cloned().unwrap_or_else(|| {
            chat.phone_number()
                .unwrap_or_else(|| chat.as_str().to_owned())
                .into()
        })
    }

    /// Names people and groups by every address they go by.
    pub fn set_titles(&mut self, titles: HashMap<ChatId, SharedString>, cx: &mut Context<Self>) {
        self.titles = titles;
        cx.notify();
    }
}
