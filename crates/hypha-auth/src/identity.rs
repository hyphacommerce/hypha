//! An identity and its credentials (design note 0008, A2): how one principal (a staff
//! member; later a customer) can prove who they are.

use core::time::Duration;

use hypha_core::{CustomerId, Email, IdentityId, Timestamp, UserId, Version, Versioned};
use serde::{Deserialize, Serialize};

use crate::{
    error::{IdentityError, RestoreError},
    password::PasswordHash,
    secret_box::Sealed,
    token::{TokenHash, hex},
};

/// Who an identity belongs to. Typed, so a login can't be linked to the wrong kind of
/// record (Medusa keeps this in untyped JSON).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Principal {
    Staff(UserId),
    /// A shopper with an account (design note 0034).
    Customer(CustomerId),
}

impl Principal {
    pub fn kind(self) -> PrincipalKind {
        match self {
            Principal::Staff(_) => PrincipalKind::Staff,
            Principal::Customer(_) => PrincipalKind::Customer,
        }
    }
}

/// Logins are unique per kind: a staff member and a customer may share an email.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrincipalKind {
    Staff,
    Customer,
}

impl PrincipalKind {
    pub fn as_str(self) -> &'static str {
        match self {
            PrincipalKind::Staff => "staff",
            PrincipalKind::Customer => "customer",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "staff" => Some(PrincipalKind::Staff),
            "customer" => Some(PrincipalKind::Customer),
            _ => None,
        }
    }
}

/// One way to prove the identity. A new login method (OIDC, passkeys) is a new variant
/// here and nothing else: storage keeps every kind as `StoredCredential`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Credential {
    /// The first factor: email and password.
    Password { email: Email, hash: PasswordHash },
    /// An authenticator app. Unconfirmed while being set up; it only counts as a
    /// second factor once a code from the app has been checked.
    Totp {
        secret: Sealed,
        confirmed: bool,
        /// The last step a code was accepted for; earlier ones are refused (replay).
        last_step: Option<u64>,
        /// Wrong second-factor codes (TOTP or recovery) since the last right one or
        /// the last lockout.
        failed_attempts: u32,
        /// While set and in the future, no second-factor code is even checked.
        locked_until: Option<Timestamp>,
    },
    /// The unused recovery codes, hashed.
    RecoveryCodes { hashes: Vec<TokenHash> },
    /// A customer's way in (design note 0034, AC1): the email a code was sent to, the
    /// code's hash, when it stops working and how many wrong guesses it took. One live
    /// code at a time; a new request replaces it. `None` hash: no code is out.
    EmailCode {
        email: Email,
        hash: Option<TokenHash>,
        expires_at: Option<Timestamp>,
        attempts: u32,
    },
}

/// What checking an email code found (`Identity::use_email_code`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeCheck {
    Right,
    Wrong,
    /// No live code: none out, expired, or used up. Answered like `Wrong` to the caller.
    None,
}

/// Ordered as an identity lists its credentials: the first factor, then the second.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CredentialKind {
    Password,
    EmailCode,
    Totp,
    RecoveryCodes,
}

impl CredentialKind {
    pub fn as_str(self) -> &'static str {
        match self {
            CredentialKind::Password => "password",
            CredentialKind::EmailCode => "email_code",
            CredentialKind::Totp => "totp",
            CredentialKind::RecoveryCodes => "recovery_codes",
        }
    }
}

impl Credential {
    pub fn kind(&self) -> CredentialKind {
        match self {
            Credential::Password { .. } => CredentialKind::Password,
            Credential::EmailCode { .. } => CredentialKind::EmailCode,
            Credential::Totp { .. } => CredentialKind::Totp,
            Credential::RecoveryCodes { .. } => CredentialKind::RecoveryCodes,
        }
    }

    /// What a login looks this credential up by, unique per principal kind and
    /// credential kind: the email for a password. Second factors have none.
    pub fn lookup(&self) -> Option<&str> {
        match self {
            Credential::Password { email, .. } | Credential::EmailCode { email, .. } => {
                Some(email.as_str())
            }
            Credential::Totp { .. } | Credential::RecoveryCodes { .. } => None,
        }
    }
}

/// A principal's credentials. The aggregate root: credentials change only through it, so
/// rules like "recovery codes need a confirmed authenticator" hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    id: IdentityId,
    version: Version,
    principal: Principal,
    credentials: Vec<Credential>,
}

impl Versioned for Identity {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

impl Identity {
    /// An identity that logs in with `email` and a password.
    pub fn with_password(
        id: IdentityId,
        principal: Principal,
        email: Email,
        hash: PasswordHash,
    ) -> Self {
        Self {
            id,
            version: Version::NEW,
            principal,
            credentials: vec![Credential::Password { email, hash }],
        }
    }

    /// An identity that signs in with a code emailed to `email` (design note 0034): no
    /// code is out yet.
    pub fn with_email_code(id: IdentityId, principal: Principal, email: Email) -> Self {
        Self {
            id,
            version: Version::NEW,
            principal,
            credentials: vec![Credential::EmailCode {
                email,
                hash: None,
                expires_at: None,
                attempts: 0,
            }],
        }
    }

    /// Wrong email codes before the code is void (design note 0034, AC1).
    pub const MAX_CODE_ATTEMPTS: u32 = 5;

    /// The email codes go to, if this identity signs in with one.
    pub fn code_email(&self) -> Option<&Email> {
        match self.find(CredentialKind::EmailCode) {
            Some(Credential::EmailCode { email, .. }) => Some(email),
            _ => None,
        }
    }

    /// Points the code credential at a new address (the customer's email changed), voiding
    /// any live code.
    pub fn set_code_email(&mut self, new: Email) -> Result<(), IdentityError> {
        match self.find_mut(CredentialKind::EmailCode) {
            Some(Credential::EmailCode {
                email,
                hash,
                expires_at,
                attempts,
            }) => {
                *email = new;
                *hash = None;
                *expires_at = None;
                *attempts = 0;
                Ok(())
            }
            _ => Err(IdentityError::NoEmailCode),
        }
    }

    /// When the live code was issued, if one is out: its `expires_at` less `lifetime`.
    /// For "no new code while the last is this young".
    pub fn code_issued_at(&self, lifetime: Duration) -> Option<Timestamp> {
        match self.find(CredentialKind::EmailCode) {
            Some(Credential::EmailCode {
                hash: Some(_),
                expires_at: Some(at),
                ..
            }) => Some(at.minus(lifetime)),
            _ => None,
        }
    }

    /// Puts a new code out, replacing any live one and starting the guesses again.
    pub fn set_email_code(
        &mut self,
        hash: TokenHash,
        expires_at: Timestamp,
    ) -> Result<(), IdentityError> {
        match self.find_mut(CredentialKind::EmailCode) {
            Some(Credential::EmailCode {
                hash: slot,
                expires_at: until,
                attempts,
                ..
            }) => {
                *slot = Some(hash);
                *until = Some(expires_at);
                *attempts = 0;
                Ok(())
            }
            _ => Err(IdentityError::NoEmailCode),
        }
    }

    /// Checks `guess` against the live code at `now` and uses it up: right, the code is
    /// gone; wrong, a guess is counted and the `MAX_CODE_ATTEMPTS`th voids the code;
    /// expired or none out, nothing to check. The caller stores the identity either way,
    /// so every guess counts.
    pub fn use_email_code(&mut self, guess: TokenHash, now: Timestamp) -> CodeCheck {
        let Some(Credential::EmailCode {
            hash,
            expires_at,
            attempts,
            ..
        }) = self.find_mut(CredentialKind::EmailCode)
        else {
            return CodeCheck::None;
        };
        let (Some(live), Some(until)) = (*hash, *expires_at) else {
            return CodeCheck::None;
        };
        if now >= until {
            *hash = None;
            *expires_at = None;
            *attempts = 0;
            return CodeCheck::None;
        }
        if live == guess {
            *hash = None;
            *expires_at = None;
            *attempts = 0;
            return CodeCheck::Right;
        }
        *attempts += 1;
        if *attempts >= Self::MAX_CODE_ATTEMPTS {
            *hash = None;
            *expires_at = None;
            *attempts = 0;
        }
        CodeCheck::Wrong
    }

    pub fn id(&self) -> IdentityId {
        self.id
    }

    pub fn principal(&self) -> Principal {
        self.principal
    }

    pub fn credentials(&self) -> &[Credential] {
        &self.credentials
    }

    fn find(&self, kind: CredentialKind) -> Option<&Credential> {
        self.credentials.iter().find(|c| c.kind() == kind)
    }

    fn find_mut(&mut self, kind: CredentialKind) -> Option<&mut Credential> {
        self.credentials.iter_mut().find(|c| c.kind() == kind)
    }

    /// The login email and password hash, if this identity has a password.
    pub fn password(&self) -> Option<(&Email, &PasswordHash)> {
        match self.find(CredentialKind::Password) {
            Some(Credential::Password { email, hash }) => Some((email, hash)),
            _ => None,
        }
    }

    /// Replaces the password hash. The login email stays.
    pub fn set_password_hash(&mut self, new_hash: PasswordHash) -> Result<(), IdentityError> {
        match self.find_mut(CredentialKind::Password) {
            Some(Credential::Password { hash, .. }) => {
                *hash = new_hash;
                Ok(())
            }
            _ => Err(IdentityError::NoPassword),
        }
    }

    /// Whether a second factor is on: a confirmed authenticator.
    pub fn mfa_enabled(&self) -> bool {
        matches!(
            self.find(CredentialKind::Totp),
            Some(Credential::Totp {
                confirmed: true,
                ..
            })
        )
    }

    /// The authenticator's sealed secret and the last step it accepted, confirmed or
    /// not.
    pub fn totp(&self) -> Option<(&Sealed, bool, Option<u64>)> {
        match self.find(CredentialKind::Totp) {
            Some(Credential::Totp {
                secret,
                confirmed,
                last_step,
                ..
            }) => Some((secret, *confirmed, *last_step)),
            _ => None,
        }
    }

    /// Wrong second-factor codes in a row before checks stop for a while
    /// (`AuthSettings::second_factor_lock`).
    pub const MAX_MFA_FAILURES: u32 = 5;

    /// Until when second-factor checks are locked, if they are at `now`.
    pub fn mfa_locked_until(&self, now: Timestamp) -> Option<Timestamp> {
        match self.find(CredentialKind::Totp) {
            Some(Credential::Totp {
                locked_until: Some(until),
                ..
            }) if now < *until => Some(*until),
            _ => None,
        }
    }

    /// Counts a wrong second-factor code. The `MAX_MFA_FAILURES`th locks checks for
    /// `lock` and starts the count again. Without an authenticator there is nothing to
    /// count against (and nothing to guess).
    pub fn record_mfa_failure(&mut self, now: Timestamp, lock: Duration) {
        if let Some(Credential::Totp {
            failed_attempts,
            locked_until,
            ..
        }) = self.find_mut(CredentialKind::Totp)
        {
            *failed_attempts += 1;
            if *failed_attempts >= Self::MAX_MFA_FAILURES {
                *failed_attempts = 0;
                *locked_until = Some(now.plus(lock));
            }
        }
    }

    fn clear_mfa_failures(&mut self) {
        if let Some(Credential::Totp {
            failed_attempts,
            locked_until,
            ..
        }) = self.find_mut(CredentialKind::Totp)
        {
            *failed_attempts = 0;
            *locked_until = None;
        }
    }

    /// Starts setting up an authenticator, replacing a setup that was never confirmed.
    pub fn start_totp(&mut self, secret: Sealed) -> Result<(), IdentityError> {
        if self.mfa_enabled() {
            return Err(IdentityError::MfaAlreadyEnabled);
        }
        self.credentials
            .retain(|c| c.kind() != CredentialKind::Totp);
        self.credentials.push(Credential::Totp {
            secret,
            confirmed: false,
            last_step: None,
            failed_attempts: 0,
            locked_until: None,
        });
        Ok(())
    }

    /// Turns the second factor on once a code for `step` checked out, with a fresh set
    /// of recovery codes.
    pub fn confirm_totp(
        &mut self,
        step: u64,
        recovery_codes: Vec<TokenHash>,
    ) -> Result<(), IdentityError> {
        match self.find_mut(CredentialKind::Totp) {
            Some(Credential::Totp {
                confirmed: true, ..
            }) => return Err(IdentityError::MfaAlreadyEnabled),
            Some(Credential::Totp {
                confirmed,
                last_step,
                failed_attempts,
                locked_until,
                ..
            }) => {
                *confirmed = true;
                *last_step = Some(step);
                *failed_attempts = 0;
                *locked_until = None;
            }
            _ => return Err(IdentityError::MfaSetupNotStarted),
        }
        self.set_recovery_codes(recovery_codes)
    }

    /// Records that a code for `step` was accepted, so it can't be used again, and
    /// clears the count of wrong codes.
    pub fn record_totp_step(&mut self, step: u64) -> Result<(), IdentityError> {
        match self.find_mut(CredentialKind::Totp) {
            Some(Credential::Totp {
                confirmed: true,
                last_step,
                ..
            }) => {
                *last_step = Some(step);
            }
            _ => return Err(IdentityError::MfaNotEnabled),
        }
        self.clear_mfa_failures();
        Ok(())
    }

    /// Uses up a recovery code. `false` if it isn't one of the unused codes.
    pub fn use_recovery_code(&mut self, code: TokenHash) -> bool {
        let Some(Credential::RecoveryCodes { hashes }) =
            self.find_mut(CredentialKind::RecoveryCodes)
        else {
            return false;
        };
        let before = hashes.len();
        hashes.retain(|h| *h != code);
        let used = hashes.len() < before;
        if used {
            self.clear_mfa_failures();
        }
        used
    }

    /// How many recovery codes are left.
    pub fn recovery_codes_left(&self) -> usize {
        match self.find(CredentialKind::RecoveryCodes) {
            Some(Credential::RecoveryCodes { hashes }) => hashes.len(),
            _ => 0,
        }
    }

    /// Replaces the recovery codes. Only with the second factor on.
    pub fn set_recovery_codes(&mut self, hashes: Vec<TokenHash>) -> Result<(), IdentityError> {
        if !self.mfa_enabled() {
            return Err(IdentityError::MfaNotEnabled);
        }
        self.credentials
            .retain(|c| c.kind() != CredentialKind::RecoveryCodes);
        self.credentials.push(Credential::RecoveryCodes { hashes });
        Ok(())
    }

    /// Removes the authenticator and the recovery codes.
    pub fn disable_mfa(&mut self) -> Result<(), IdentityError> {
        if !self.mfa_enabled() {
            return Err(IdentityError::MfaNotEnabled);
        }
        self.credentials.retain(|c| {
            !matches!(
                c.kind(),
                CredentialKind::Totp | CredentialKind::RecoveryCodes
            )
        });
        Ok(())
    }
}

// ---------- storage ----------

/// One credential as storage keeps it (design note 0008, A2): a kind, the lookup a
/// login finds it by, and the rest as JSON. A new credential kind needs no new column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredCredential {
    pub kind: String,
    pub lookup: Option<String>,
    pub data: serde_json::Value,
}

/// Plain-data form of an `Identity` for storage adapters (design note 0001, 3.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentitySnapshot {
    pub id: IdentityId,
    pub version: Version,
    pub principal: Principal,
    pub credentials: Vec<StoredCredential>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PasswordData {
    hash: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EmailCodeData {
    /// Hex of the SHA-256 of the code; absent while no code is out.
    hash: Option<String>,
    /// Microseconds since the epoch.
    expires_at: Option<i64>,
    #[serde(default)]
    attempts: u32,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TotpData {
    /// Hex of the sealed bytes.
    secret: String,
    confirmed: bool,
    last_step: Option<u64>,
    #[serde(default)]
    failed_attempts: u32,
    /// Microseconds since the epoch.
    #[serde(default)]
    locked_until: Option<i64>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecoveryCodesData {
    /// Hex of each SHA-256.
    hashes: Vec<String>,
}

impl Credential {
    fn to_stored(&self) -> StoredCredential {
        // Serializing these plain structs to a `Value` can't fail.
        let data = match self {
            Credential::Password { hash, .. } => serde_json::json!(PasswordData {
                hash: hash.as_str().to_string()
            }),
            Credential::Totp {
                secret,
                confirmed,
                last_step,
                failed_attempts,
                locked_until,
            } => serde_json::json!(TotpData {
                secret: hex(secret.as_bytes()),
                confirmed: *confirmed,
                last_step: *last_step,
                failed_attempts: *failed_attempts,
                locked_until: locked_until.map(Timestamp::unix_micros),
            }),
            Credential::RecoveryCodes { hashes } => serde_json::json!(RecoveryCodesData {
                hashes: hashes.iter().map(|h| hex(h.as_bytes())).collect(),
            }),
            Credential::EmailCode {
                hash,
                expires_at,
                attempts,
                ..
            } => serde_json::json!(EmailCodeData {
                hash: hash.map(|h| hex(h.as_bytes())),
                expires_at: expires_at.map(Timestamp::unix_micros),
                attempts: *attempts,
            }),
        };
        StoredCredential {
            kind: self.kind().as_str().to_string(),
            lookup: self.lookup().map(str::to_string),
            data,
        }
    }

    fn from_stored(stored: StoredCredential) -> Result<Credential, RestoreError> {
        let bad = |what: &str| RestoreError::BadCredential(format!("{}: {what}", stored.kind));
        match stored.kind.as_str() {
            "password" => {
                let data: PasswordData =
                    serde_json::from_value(stored.data.clone()).map_err(|e| bad(&e.to_string()))?;
                let raw = stored.lookup.as_deref().ok_or_else(|| bad("no email"))?;
                let email = Email::new(raw).map_err(|e| bad(&e.to_string()))?;
                // The unique index compares stored text, so it must be the normalized form.
                if email.as_str() != raw {
                    return Err(bad("email not normalized"));
                }
                Ok(Credential::Password {
                    email,
                    hash: PasswordHash::from_phc(data.hash),
                })
            }
            "email_code" => {
                let data: EmailCodeData =
                    serde_json::from_value(stored.data.clone()).map_err(|e| bad(&e.to_string()))?;
                let raw = stored.lookup.as_deref().ok_or_else(|| bad("no email"))?;
                let email = Email::new(raw).map_err(|e| bad(&e.to_string()))?;
                if email.as_str() != raw {
                    return Err(bad("email not normalized"));
                }
                let hash = data
                    .hash
                    .as_deref()
                    .map(|h| {
                        unhex(h)
                            .and_then(|b| <[u8; 32]>::try_from(b).ok())
                            .map(TokenHash::from_bytes)
                            .ok_or_else(|| bad("hash"))
                    })
                    .transpose()?;
                Ok(Credential::EmailCode {
                    email,
                    hash,
                    expires_at: data.expires_at.map(Timestamp::from_unix_micros),
                    attempts: data.attempts,
                })
            }
            "totp" => {
                let data: TotpData =
                    serde_json::from_value(stored.data.clone()).map_err(|e| bad(&e.to_string()))?;
                if stored.lookup.is_some() {
                    return Err(bad("unexpected lookup"));
                }
                Ok(Credential::Totp {
                    secret: Sealed::from_bytes(unhex(&data.secret).ok_or_else(|| bad("secret"))?),
                    confirmed: data.confirmed,
                    last_step: data.last_step,
                    failed_attempts: data.failed_attempts,
                    locked_until: data.locked_until.map(Timestamp::from_unix_micros),
                })
            }
            "recovery_codes" => {
                let data: RecoveryCodesData =
                    serde_json::from_value(stored.data.clone()).map_err(|e| bad(&e.to_string()))?;
                if stored.lookup.is_some() {
                    return Err(bad("unexpected lookup"));
                }
                let hashes = data
                    .hashes
                    .iter()
                    .map(|h| {
                        unhex(h)
                            .and_then(|b| <[u8; 32]>::try_from(b).ok())
                            .map(TokenHash::from_bytes)
                    })
                    .collect::<Option<Vec<_>>>()
                    .ok_or_else(|| bad("hash"))?;
                Ok(Credential::RecoveryCodes { hashes })
            }
            _ => Err(RestoreError::UnknownCredentialKind(stored.kind)),
        }
    }
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

impl Identity {
    pub fn snapshot(&self) -> IdentitySnapshot {
        IdentitySnapshot {
            id: self.id,
            version: self.version,
            principal: self.principal,
            credentials: self.credentials.iter().map(Credential::to_stored).collect(),
        }
    }

    /// Rebuilds an identity from storage, re-checking the rules: at most one credential
    /// of each kind, and recovery codes only beside a confirmed authenticator. Storage
    /// may return credentials in any order; they come back in `CredentialKind` order,
    /// the order the methods above add them in.
    pub fn restore(s: IdentitySnapshot) -> Result<Identity, RestoreError> {
        let mut credentials = s
            .credentials
            .into_iter()
            .map(Credential::from_stored)
            .collect::<Result<Vec<_>, _>>()?;
        credentials.sort_by_key(Credential::kind);
        for (i, c) in credentials.iter().enumerate() {
            if credentials[..i].iter().any(|o| o.kind() == c.kind()) {
                return Err(RestoreError::DuplicateCredential(c.kind().as_str()));
            }
        }
        let identity = Identity {
            id: s.id,
            version: s.version,
            principal: s.principal,
            credentials,
        };
        if identity.find(CredentialKind::RecoveryCodes).is_some() && !identity.mfa_enabled() {
            return Err(RestoreError::RecoveryCodesWithoutMfa);
        }
        Ok(identity)
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;

    const LOCK: Duration = Duration::from_secs(15 * 60);

    fn identity() -> Identity {
        Identity::with_password(
            IdentityId::from_uuid(Uuid::from_u128(1)),
            Principal::Staff(UserId::from_uuid(Uuid::from_u128(2))),
            Email::new("ana@example.com").unwrap(),
            PasswordHash::from_phc("$argon2id$fake".to_string()),
        )
    }

    fn h(n: u8) -> TokenHash {
        TokenHash::from_bytes([n; 32])
    }

    fn with_mfa() -> Identity {
        let mut i = identity();
        i.start_totp(Sealed::from_bytes(vec![1, 2, 3])).unwrap();
        i.confirm_totp(10, vec![h(1), h(2)]).unwrap();
        i
    }

    fn at(secs: i64) -> Timestamp {
        Timestamp::from_unix_micros(secs * 1_000_000)
    }

    fn customer_identity() -> Identity {
        Identity::with_email_code(
            IdentityId::from_uuid(Uuid::from_u128(3)),
            Principal::Customer(hypha_core::CustomerId::from_uuid(Uuid::from_u128(4))),
            Email::new("nina@example.com").unwrap(),
        )
    }

    #[test]
    fn an_email_code_is_used_once_and_voided_after_five_wrong_guesses() {
        let mut i = customer_identity();
        assert_eq!(i.use_email_code(h(1), at(0)), CodeCheck::None);
        assert_eq!(i.code_issued_at(Duration::from_secs(600)), None);
        i.set_email_code(h(1), at(600)).unwrap();
        assert_eq!(
            i.code_issued_at(Duration::from_secs(600)),
            Some(at(0)),
            "issued when it expires less its life"
        );
        assert_eq!(i.use_email_code(h(2), at(10)), CodeCheck::Wrong);
        assert_eq!(i.use_email_code(h(1), at(10)), CodeCheck::Right);
        assert_eq!(i.use_email_code(h(1), at(10)), CodeCheck::None, "used up");

        i.set_email_code(h(5), at(600)).unwrap();
        for _ in 0..Identity::MAX_CODE_ATTEMPTS {
            assert_eq!(i.use_email_code(h(9), at(10)), CodeCheck::Wrong);
        }
        assert_eq!(i.use_email_code(h(5), at(10)), CodeCheck::None, "voided");

        i.set_email_code(h(6), at(600)).unwrap();
        assert_eq!(i.use_email_code(h(6), at(600)), CodeCheck::None, "expired");
        assert_eq!(
            identity().set_email_code(h(1), at(1)),
            Err(IdentityError::NoEmailCode)
        );
        assert_eq!(i.code_email().map(Email::as_str), Some("nina@example.com"));
        assert_eq!(i.credentials()[0].lookup(), Some("nina@example.com"));
    }

    #[test]
    fn an_email_code_identity_round_trips_through_storage() {
        let mut i = customer_identity();
        i.set_email_code(h(7), at(600)).unwrap();
        i.use_email_code(h(1), at(1));
        assert_eq!(Identity::restore(i.snapshot()), Ok(i));
        assert_eq!(
            Identity::restore(customer_identity().snapshot()),
            Ok(customer_identity())
        );
    }

    #[test]
    fn a_new_identity_has_a_password_and_no_second_factor() {
        let i = identity();
        assert_eq!(
            i.password().map(|(e, _)| e.as_str()),
            Some("ana@example.com")
        );
        assert!(!i.mfa_enabled());
        assert_eq!(i.credentials()[0].lookup(), Some("ana@example.com"));
    }

    #[test]
    fn an_unconfirmed_authenticator_is_not_a_second_factor() {
        let mut i = identity();
        i.start_totp(Sealed::from_bytes(vec![1])).unwrap();
        assert!(!i.mfa_enabled());
        // Starting again replaces it.
        i.start_totp(Sealed::from_bytes(vec![2])).unwrap();
        assert_eq!(i.totp().map(|(s, ..)| s.as_bytes().to_vec()), Some(vec![2]));
        assert_eq!(i.credentials().len(), 2);
    }

    #[test]
    fn confirming_turns_mfa_on_with_recovery_codes() {
        let i = with_mfa();
        assert!(i.mfa_enabled());
        assert_eq!(
            i.totp().map(|(_, c, step)| (c, step)),
            Some((true, Some(10)))
        );
        assert_eq!(i.recovery_codes_left(), 2);
    }

    #[test]
    fn confirming_needs_a_setup_and_happens_once() {
        assert_eq!(
            identity().confirm_totp(1, vec![]),
            Err(IdentityError::MfaSetupNotStarted)
        );
        assert_eq!(
            with_mfa().confirm_totp(11, vec![]),
            Err(IdentityError::MfaAlreadyEnabled)
        );
        assert_eq!(
            with_mfa().start_totp(Sealed::from_bytes(vec![])),
            Err(IdentityError::MfaAlreadyEnabled)
        );
    }

    #[test]
    fn the_fifth_wrong_code_locks_checks_for_a_while() {
        let now = Timestamp::UNIX_EPOCH;
        let mut i = with_mfa();
        for _ in 0..4 {
            i.record_mfa_failure(now, LOCK);
        }
        assert_eq!(i.mfa_locked_until(now), None);
        i.record_mfa_failure(now, LOCK);
        let until = now.plus(LOCK);
        assert_eq!(i.mfa_locked_until(now), Some(until));
        assert_eq!(i.mfa_locked_until(until), None);
    }

    #[test]
    fn a_right_code_clears_the_count() {
        let now = Timestamp::UNIX_EPOCH;
        let mut i = with_mfa();
        for _ in 0..4 {
            i.record_mfa_failure(now, LOCK);
        }
        assert!(i.use_recovery_code(h(1)));
        for _ in 0..4 {
            i.record_mfa_failure(now, LOCK);
        }
        assert_eq!(i.mfa_locked_until(now), None);
        i.record_totp_step(11).unwrap();
        for _ in 0..4 {
            i.record_mfa_failure(now, LOCK);
        }
        assert_eq!(i.mfa_locked_until(now), None);
    }

    #[test]
    fn the_lockout_survives_storage() {
        let now = Timestamp::UNIX_EPOCH;
        let mut i = with_mfa();
        for _ in 0..7 {
            i.record_mfa_failure(now, LOCK);
        }
        let back = Identity::restore(i.snapshot()).unwrap();
        assert_eq!(back.mfa_locked_until(now), i.mfa_locked_until(now));
        assert_eq!(back, i);
    }

    #[test]
    fn totp_data_from_before_the_lockout_still_reads() {
        let mut s = with_mfa().snapshot();
        let totp = s.credentials.iter_mut().find(|c| c.kind == "totp").unwrap();
        let data = totp.data.as_object_mut().unwrap();
        data.remove("failed_attempts");
        data.remove("locked_until");
        assert!(Identity::restore(s).is_ok());
    }

    #[test]
    fn a_recovery_code_works_once() {
        let mut i = with_mfa();
        assert!(i.use_recovery_code(h(1)));
        assert!(!i.use_recovery_code(h(1)));
        assert!(!i.use_recovery_code(h(9)));
        assert_eq!(i.recovery_codes_left(), 1);
    }

    #[test]
    fn disabling_removes_both_factors_and_keeps_the_password() {
        let mut i = with_mfa();
        i.disable_mfa().unwrap();
        assert!(!i.mfa_enabled());
        assert_eq!(i.credentials().len(), 1);
        assert_eq!(i.disable_mfa(), Err(IdentityError::MfaNotEnabled));
    }

    #[test]
    fn recovery_codes_need_mfa() {
        assert_eq!(
            identity().set_recovery_codes(vec![h(1)]),
            Err(IdentityError::MfaNotEnabled)
        );
    }

    #[test]
    fn snapshot_round_trips() {
        let i = with_mfa();
        assert_eq!(Identity::restore(i.snapshot()), Ok(i));
    }

    #[test]
    fn restore_puts_credentials_in_kind_order() {
        let i = with_mfa();
        let mut s = i.snapshot();
        s.credentials.reverse();
        assert_eq!(Identity::restore(s), Ok(i));
    }

    #[test]
    fn restore_refuses_two_credentials_of_a_kind() {
        let mut s = identity().snapshot();
        s.credentials.push(s.credentials[0].clone());
        assert_eq!(
            Identity::restore(s),
            Err(RestoreError::DuplicateCredential("password"))
        );
    }

    #[test]
    fn restore_refuses_recovery_codes_without_mfa() {
        let mut s = with_mfa().snapshot();
        s.credentials.retain(|c| c.kind != "totp");
        assert_eq!(
            Identity::restore(s),
            Err(RestoreError::RecoveryCodesWithoutMfa)
        );
    }

    #[test]
    fn restore_refuses_unknown_kinds_and_bad_data() {
        let mut s = identity().snapshot();
        s.credentials[0].kind = "carrier_pigeon".to_string();
        assert_eq!(
            Identity::restore(s),
            Err(RestoreError::UnknownCredentialKind(
                "carrier_pigeon".to_string()
            ))
        );
        let mut s = identity().snapshot();
        s.credentials[0].data = serde_json::json!({ "hash": 1 });
        assert!(matches!(
            Identity::restore(s),
            Err(RestoreError::BadCredential(_))
        ));
        let mut s = identity().snapshot();
        s.credentials[0].lookup = Some("Ana@Example.com".to_string());
        assert!(matches!(
            Identity::restore(s),
            Err(RestoreError::BadCredential(_))
        ));
    }
}
