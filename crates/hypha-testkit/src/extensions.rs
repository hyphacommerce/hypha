use std::sync::Mutex;

use hypha_core::{AppKey, ExtensionCallId, Page, PageRequest, RepoError, Timestamp};
use hypha_extensions::{ExtensionCall, ExtensionCallRepo, keys};

/// The call log in memory: calls by ID.
#[derive(Default)]
pub struct InMemoryExtensionCallRepo {
    calls: Mutex<Vec<ExtensionCall>>,
}

impl InMemoryExtensionCallRepo {
    /// Every call recorded, oldest first: for tests that check what was called.
    pub fn all(&self) -> Vec<ExtensionCall> {
        let mut calls = self.calls.lock().unwrap().clone();
        calls.sort_by_key(|c| c.id);
        calls
    }
}

impl ExtensionCallRepo for InMemoryExtensionCallRepo {
    async fn insert(&self, call: &ExtensionCall) -> Result<(), RepoError> {
        let mut calls = self.calls.lock().unwrap();
        if calls.iter().any(|c| c.id == call.id) {
            return Err(RepoError::Duplicate(keys::EXTENSION_CALL_ID));
        }
        calls.push(call.clone());
        Ok(())
    }

    async fn list_for_app(
        &self,
        app: &AppKey,
        page: PageRequest<ExtensionCallId>,
    ) -> Result<Page<ExtensionCall>, RepoError> {
        let mut found: Vec<ExtensionCall> = self
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| &c.app == app && page.after.is_none_or(|after| c.id < after))
            .cloned()
            .collect();
        found.sort_by_key(|c| std::cmp::Reverse(c.id));
        found.truncate(page.limit.get() as usize + 1);
        Ok(Page::from_overfetch(found, page.limit))
    }

    async fn prune(&self, before: Timestamp, limit: u32) -> Result<u32, RepoError> {
        let mut calls = self.calls.lock().unwrap();
        let mut old: Vec<ExtensionCallId> = calls
            .iter()
            .filter(|c| c.started_at < before)
            .map(|c| c.id)
            .collect();
        old.truncate(limit as usize);
        calls.retain(|c| !old.contains(&c.id));
        Ok(old.len() as u32)
    }
}
