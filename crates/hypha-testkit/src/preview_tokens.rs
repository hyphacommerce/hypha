use std::{collections::HashMap, sync::Mutex};

use hypha_core::{PreviewToken, PreviewTokenRepo, RepoError, Timestamp, preview_token::keys};

/// Preview tokens in memory, by hash.
#[derive(Default)]
pub struct InMemoryPreviewTokenRepo {
    tokens: Mutex<HashMap<[u8; 32], PreviewToken>>,
}

impl PreviewTokenRepo for InMemoryPreviewTokenRepo {
    async fn insert(&self, token: &PreviewToken) -> Result<(), RepoError> {
        let mut tokens = self.tokens.lock().unwrap();
        if tokens.contains_key(&token.hash) {
            return Err(RepoError::Duplicate(keys::PREVIEW_TOKEN));
        }
        tokens.insert(token.hash, token.clone());
        Ok(())
    }

    async fn get(&self, hash: &[u8; 32]) -> Result<Option<PreviewToken>, RepoError> {
        Ok(self.tokens.lock().unwrap().get(hash).cloned())
    }

    async fn mark_used(&self, hash: &[u8; 32], at: Timestamp) -> Result<bool, RepoError> {
        let mut tokens = self.tokens.lock().unwrap();
        match tokens.get_mut(hash) {
            Some(token) if token.used_at.is_none() => {
                token.used_at = Some(at);
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    async fn prune(&self, before: Timestamp, limit: u32) -> Result<u32, RepoError> {
        let mut tokens = self.tokens.lock().unwrap();
        let old: Vec<[u8; 32]> = tokens
            .values()
            .filter(|t| t.expires_at < before)
            .map(|t| t.hash)
            .take(limit as usize)
            .collect();
        for hash in &old {
            tokens.remove(hash);
        }
        Ok(old.len() as u32)
    }
}
