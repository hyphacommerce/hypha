//! A deterministic environment for auth commands: `hypha_auth::AuthEnv` plus a
//! `Notifier` that keeps what it was asked to send.

use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use hypha_auth::{
    Argon2id, AuthSettings, HasAuthSettings, HashError, Password, PasswordHash, PasswordHasher,
    SealError, Sealed, SecretBox, XChaChaBox,
};
use hypha_core::{Clock, IdGenerator, Notifier, NotifyError, Random, Timestamp};
use hypha_users::StaffEmail;
use uuid::Uuid;

use crate::{FixedClock, SequentialIds};

/// IDs count up, the clock stands still until moved, randomness is a fixed sequence,
/// passwords hash with cheap Argon2 parameters, and sent messages are kept in
/// `sent`. Every token and code a test sees is therefore the same on every run.
pub struct TestEnv {
    pub ids: SequentialIds,
    pub clock: FixedClock,
    /// The defaults unless a test sets its own.
    pub auth: AuthSettings,
    random: AtomicU64,
    secret_box: XChaChaBox,
    sent: Mutex<Vec<StaffEmail>>,
    failing: AtomicBool,
}

impl Default for TestEnv {
    fn default() -> Self {
        Self {
            ids: SequentialIds::default(),
            clock: FixedClock::default(),
            auth: AuthSettings::default(),
            random: AtomicU64::new(0),
            secret_box: XChaChaBox::new([7; 32]),
            sent: Mutex::new(Vec::new()),
            failing: AtomicBool::new(false),
        }
    }
}

/// Argon2id with the smallest parameters, so tests stay fast.
const CHEAP: Argon2id = Argon2id::new(8, 1, 1);

impl TestEnv {
    /// Every message sent so far, oldest first.
    pub fn sent(&self) -> Vec<StaffEmail> {
        self.sent.lock().unwrap().clone()
    }

    /// The token in the last message sent.
    pub fn last_token(&self) -> Option<String> {
        self.sent.lock().unwrap().last().map(|m| match m {
            StaffEmail::Invite { token, .. } | StaffEmail::PasswordReset { token, .. } => {
                token.clone()
            }
        })
    }

    /// Makes every later send fail (or work again).
    pub fn fail_sending(&self, failing: bool) {
        self.failing.store(failing, Ordering::Relaxed);
    }

    /// Moves the clock `secs` seconds on.
    pub fn advance(&self, secs: i64) {
        self.clock.set(self.clock.now().plus_secs(secs));
    }

    /// Moves the clock `duration` on.
    pub fn advance_by(&self, duration: std::time::Duration) {
        self.clock.set(self.clock.now().plus(duration));
    }

    pub fn now(&self) -> Timestamp {
        self.clock.now()
    }
}

impl IdGenerator for TestEnv {
    fn new_uuid(&self) -> Uuid {
        self.ids.new_uuid()
    }
}

impl HasAuthSettings for TestEnv {
    fn auth_settings(&self) -> &AuthSettings {
        &self.auth
    }
}

impl Clock for TestEnv {
    fn now(&self) -> Timestamp {
        self.clock.now()
    }
}

impl Random for TestEnv {
    /// SplitMix64 over a counter: different bytes on every call, the same sequence on
    /// every run. Never use anything like it outside tests.
    fn fill(&self, bytes: &mut [u8]) {
        for chunk in bytes.chunks_mut(8) {
            let mut z = self
                .random
                .fetch_add(0x9E37_79B9_7F4A_7C15, Ordering::Relaxed)
                .wrapping_add(0x9E37_79B9_7F4A_7C15);
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^= z >> 31;
            chunk.copy_from_slice(&z.to_le_bytes()[..chunk.len()]);
        }
    }
}

impl PasswordHasher for TestEnv {
    async fn hash_password(
        &self,
        password: &Password,
        salt: [u8; 16],
    ) -> Result<PasswordHash, HashError> {
        CHEAP.hash_now(password, salt)
    }

    async fn verify_password(
        &self,
        password: &Password,
        hash: &PasswordHash,
    ) -> Result<bool, HashError> {
        CHEAP.verify_now(password, hash)
    }
}

impl SecretBox for TestEnv {
    fn seal(&self, plaintext: &[u8], context: &[u8], nonce: [u8; 24]) -> Sealed {
        self.secret_box.seal(plaintext, context, nonce)
    }

    fn open(&self, sealed: &Sealed, context: &[u8]) -> Result<Vec<u8>, SealError> {
        self.secret_box.open(sealed, context)
    }
}

impl Notifier<StaffEmail> for TestEnv {
    async fn notify(&self, message: StaffEmail) -> Result<(), NotifyError> {
        if self.failing.load(Ordering::Relaxed) {
            return Err(NotifyError("the test made sending fail".to_string()));
        }
        self.sent.lock().unwrap().push(message);
        Ok(())
    }
}
