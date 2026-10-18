//! How callers prove who they are (design note 0008): identities and their credentials,
//! sessions, second factors, API keys and password resets.
//!
//! A leaf domain: it knows principals only by their core IDs (`UserId`), and
//! `hypha-users` composes it with staff users and roles.

pub mod api_key;
pub mod commands;
pub mod error;
pub mod identity;
pub mod password;
pub mod password_reset;
pub mod recovery;
pub mod repo;
pub mod secret_box;
pub mod session;
pub mod settings;
pub mod token;
pub mod totp;

pub use api_key::{ApiKey, ApiKeyKind, ApiKeyParts, ApiKeySnapshot};
pub use commands::{
    AuthEnv, ChannelDirectory, CommandError, CreateApiKey, LoginStep, MfaMethod, TotpSetup,
    authenticate_api_key, authenticate_session, change_password, check_password, confirm_totp,
    create_api_key, disable_mfa, finish_login, get_api_key, hash_new_password, list_api_keys,
    logout, new_active_session, open_session, prepare_login, regenerate_recovery_codes,
    require_grantable, reset_password, revoke_api_key, session_token_hash, start_password_reset,
    start_totp, withdraw_password_resets,
};
pub use error::{ApiKeyError, IdentityError, RestoreError};
pub use identity::{
    Credential, CredentialKind, Identity, IdentitySnapshot, Principal, PrincipalKind,
    StoredCredential,
};
pub use password::{Argon2id, HashError, Password, PasswordError, PasswordHash, PasswordHasher};
pub use password_reset::{PasswordReset, PasswordResetSnapshot};
pub use repo::{
    ApiKeyRepo, HasApiKeys, HasIdentities, HasPasswordResets, HasSessions, IdentityRepo,
    PasswordResetRepo, SessionRepo, StageIdentities, StagePasswordResets, StageSessions, keys,
};
pub use secret_box::{SealError, Sealed, SecretBox, XChaChaBox};
pub use session::{Session, SessionSnapshot, SessionState};
pub use settings::{AuthSettings, HasAuthSettings};
pub use token::{NewToken, TokenHash, TokenKind};
