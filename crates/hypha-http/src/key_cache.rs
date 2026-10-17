//! Publishable keys authenticated recently (design note 0025, §6): a storefront sends
//! the same key with every request, and looking it up each time was a query per
//! request. Only publishable keys: they're public by design (0008) and grant reads and
//! carts in one channel. Secret keys and staff sessions are read fresh every time, so
//! revoking one, or changing a role's scopes, applies to the next request.
//!
//! Entries live for [`KeyCache::TTL`] and are keyed by the token's SHA-256, so the
//! cache never holds a key itself. Revoking any key clears it, so on one instance a
//! revoked publishable key stops at once; other instances stop within the TTL.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use hypha_core::{Ctx, Timestamp};
use sha2::{Digest, Sha256};

#[derive(Default)]
pub struct KeyCache {
    entries: Mutex<HashMap<[u8; 32], (Ctx, Timestamp)>>,
}

impl KeyCache {
    /// How long a lookup is reused.
    pub const TTL: Duration = Duration::from_secs(30);
    /// Past this many keys the cache starts over: a store has a handful, so more means
    /// someone is sending many keys, and none of them should cost memory.
    const CAPACITY: usize = 256;

    /// The `Ctx` `token` authenticated as, if that was less than [`Self::TTL`] ago.
    pub fn get(&self, token: &str, now: Timestamp) -> Option<Ctx> {
        let entries = self.lock();
        let (ctx, at) = entries.get(&digest(token))?;
        // The host's clock is the wall clock: one set back makes the age negative, and
        // that entry counts as expired rather than as fresh for as long as it's behind.
        let age = now.unix_micros().saturating_sub(at.unix_micros());
        (0..Self::TTL.as_micros() as i64)
            .contains(&age)
            .then(|| ctx.clone())
    }

    pub fn put(&self, token: &str, ctx: &Ctx, now: Timestamp) {
        let mut entries = self.lock();
        if entries.len() >= Self::CAPACITY {
            entries.clear();
        }
        entries.insert(digest(token), (ctx.clone(), now));
    }

    /// Forgets every key: after a revocation.
    pub fn clear(&self) {
        self.lock().clear();
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<[u8; 32], (Ctx, Timestamp)>> {
        // A poisoned lock still holds whole entries; a panic elsewhere can't tear one.
        self.entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn digest(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use hypha_core::{Actor, ApiKeyId, Scope};
    use uuid::Uuid;

    fn ctx() -> Ctx {
        Ctx::new(
            Actor::ApiKey(ApiKeyId::from_uuid(Uuid::from_u128(1))),
            vec![Scope::ProductsRead],
        )
    }

    #[test]
    fn a_key_is_reused_within_the_ttl() {
        let cache = KeyCache::default();
        let start = Timestamp::from_unix_micros(1_000_000);
        cache.put("hypha_pk_a", &ctx(), start);
        assert_eq!(
            cache.get("hypha_pk_a", start.plus(Duration::from_secs(29))),
            Some(ctx())
        );
    }

    #[test]
    fn a_key_is_looked_up_again_after_the_ttl() {
        let cache = KeyCache::default();
        let start = Timestamp::from_unix_micros(1_000_000);
        cache.put("hypha_pk_a", &ctx(), start);
        assert_eq!(cache.get("hypha_pk_a", start.plus(KeyCache::TTL)), None);
    }

    #[test]
    fn a_clock_set_back_expires_the_key() {
        let cache = KeyCache::default();
        let start = Timestamp::from_unix_micros(3_600_000_000);
        cache.put("hypha_pk_a", &ctx(), start);
        assert_eq!(
            cache.get("hypha_pk_a", Timestamp::from_unix_micros(1_000_000)),
            None
        );
    }

    #[test]
    fn another_key_isnt_found() {
        let cache = KeyCache::default();
        let start = Timestamp::from_unix_micros(1_000_000);
        cache.put("hypha_pk_a", &ctx(), start);
        assert_eq!(cache.get("hypha_pk_b", start), None);
    }

    #[test]
    fn clearing_forgets_every_key() {
        let cache = KeyCache::default();
        let start = Timestamp::from_unix_micros(1_000_000);
        cache.put("hypha_pk_a", &ctx(), start);
        cache.clear();
        assert_eq!(cache.get("hypha_pk_a", start), None);
    }
}
