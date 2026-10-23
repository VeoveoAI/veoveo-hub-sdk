//! Tool descriptions a server changes while it runs.
//!
//! A server whose tool descriptions follow live state, such as the version a tool
//! reports on, gives its routes a [`ToolDescriptions`] through
//! [`RoutesBuilder::descriptions`](super::RoutesBuilder::descriptions). The host then
//! serves each tool's current description in `tools/list`, advertises
//! `tools.listChanged`, and sends `notifications/tools/list_changed` on every accepted
//! `subscriptions/listen` after each change, so the Hub refreshes its catalog and tells
//! its clients. A tool list is shared by every caller of the server: a description
//! names no caller's own state.
use std::{
    collections::BTreeMap,
    sync::{Arc, RwLock},
};

use tokio::sync::watch;

use crate::calls::ToolCall;

/// The current descriptions of a server's tools that change while it runs, and the
/// changes its listeners wait on. Clones share one set.
#[derive(Clone)]
pub struct ToolDescriptions {
    inner: Arc<Inner>,
}

struct Inner {
    current: RwLock<BTreeMap<&'static str, Arc<str>>>,
    /// Counts changes; a listener wakes when it moves.
    changes: watch::Sender<u64>,
}

impl Default for ToolDescriptions {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for ToolDescriptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolDescriptions").finish_non_exhaustive()
    }
}

impl ToolDescriptions {
    /// No changed descriptions: every tool keeps the one its route registered.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                current: RwLock::new(BTreeMap::new()),
                changes: watch::Sender::new(0),
            }),
        }
    }

    /// Serves `description` for the tool of `R` from now on. When it differs from the
    /// one served before, the server's listeners are told that its tool list changed.
    /// Returns whether it changed.
    pub fn set<R: ToolCall>(&self, description: impl Into<String>) -> bool {
        let description: Arc<str> = description.into().into();
        {
            let mut current = self
                .inner
                .current
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if current.get(R::TOOL) == Some(&description) {
                return false;
            }
            current.insert(R::TOOL, description);
        }
        self.inner.changes.send_modify(|count| *count += 1);
        true
    }

    /// The description `set` gave the tool `name`, if any.
    pub(super) fn get(&self, name: &str) -> Option<Arc<str>> {
        self.inner
            .current
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(name)
            .cloned()
    }

    /// Changes after now.
    pub(super) fn changes(&self) -> watch::Receiver<u64> {
        self.inner.changes.subscribe()
    }
}
