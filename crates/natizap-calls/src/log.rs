//! The account's call history, kept in step with WhatsApp in the
//! background.

use std::collections::HashMap;
use std::path::PathBuf;

use gpui::{AppContext as _, Context, Entity, SharedString, Task};
use natizap_protocol::{CallFollower, CallRecord, ChatId, Protocol};

use crate::group::{CallGroup, collapse};
use crate::store::CallStore;

/// Every call that rang on the account's devices or was made from its
/// phone, newest first.
pub struct CallLog {
    calls: Vec<CallRecord>,
    /// What people and groups are called, by every address they go by.
    titles: HashMap<ChatId, SharedString>,
    _tasks: [Task<()>; 2],
}

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
            let mut follower = CallFollower::default();
            loop {
                match store.recent() {
                    Ok(calls) => {
                        if lists_tx.send(calls).await.is_err() {
                            return;
                        }
                    }
                    Err(error) => tracing::error!(%error, "could not read the call history"),
                }
                // Waits for the next call that ended.
                let ended = loop {
                    let Ok(event) = events.recv().await else {
                        return;
                    };
                    let ended = follower.hear(&event);
                    if !ended.is_empty() {
                        break ended;
                    }
                };
                if let Err(error) = store.keep(&ended) {
                    tracing::error!(%error, "could not keep calls");
                }
            }
        });
        let apply = cx.spawn(async move |this, cx| {
            while let Ok(calls) = lists.recv().await {
                if this
                    .update(cx, |log, cx| {
                        log.calls = calls;
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
            titles: HashMap::new(),
            _tasks: [work, apply],
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
