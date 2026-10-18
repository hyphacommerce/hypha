//! `hypha-auth`'s repositories: identities with their credentials, sessions, API keys
//! and password resets (design note 0008).

use hypha_auth::{
    ApiKey, ApiKeyKind, ApiKeyRepo, ApiKeySnapshot, CredentialKind, Identity, IdentityRepo,
    IdentitySnapshot, PasswordReset, PasswordResetRepo, PasswordResetSnapshot, Principal,
    PrincipalKind, Session, SessionRepo, SessionSnapshot, SessionState, StoredCredential,
    TokenHash,
};
use hypha_core::{
    ApiKeyId, ChannelId, IdentityId, PasswordResetId, RepoError, SessionId, Timestamp, UserId,
    Version, Versioned,
};
use tokio_postgres::{Row, types::Type};
use uuid::Uuid;

use crate::{
    columns::{
        Prefixed, actor_from_db, actor_to_db, col, micros, opt_micros, opt_ts, prefixed_columns,
        scopes_from_db, scopes_to_db, token_hash_from_db, ts, version_from_db, version_to_db,
    },
    error::{corrupt, repo_error},
    store::Queries,
    store::{PgConnection, PgStore, consistent_read, in_transaction},
};

fn principal_to_db(p: Principal) -> (&'static str, Uuid) {
    match p {
        Principal::Staff(id) => (p.kind().as_str(), id.as_uuid()),
    }
}

fn principal_from_db(kind: &str, id: Uuid) -> Result<Principal, RepoError> {
    match PrincipalKind::parse(kind) {
        Some(PrincipalKind::Staff) => Ok(Principal::Staff(UserId::from_uuid(id))),
        None => Err(corrupt(format!("unknown principal kind {kind:?}"))),
    }
}

// ---------- identities ----------

impl<C: PgConnection> IdentityRepo for PgStore<C> {
    async fn get(&self, id: IdentityId) -> Result<Option<Identity>, RepoError> {
        let mut conn = self.conn().await;
        consistent_read!(conn, |tx| load_identity(tx, id.as_uuid()).await)
    }

    async fn get_by_principal(&self, principal: Principal) -> Result<Option<Identity>, RepoError> {
        let (kind, pid) = principal_to_db(principal);
        let mut conn = self.conn().await;
        consistent_read!(conn, |tx| {
            let row = tx
                .query_typed_opt(
                    "SELECT id FROM identities WHERE principal_kind = $1 AND principal_id = $2",
                    &[(&kind, Type::TEXT), (&pid, Type::UUID)],
                )
                .await
                .map_err(repo_error)?;
            match row {
                Some(row) => load_identity(tx, col(&row, "id")?).await,
                None => Ok(None),
            }
        })
    }

    async fn find_login(
        &self,
        principal: PrincipalKind,
        credential: CredentialKind,
        lookup: &str,
    ) -> Result<Option<Identity>, RepoError> {
        let mut conn = self.conn().await;
        consistent_read!(conn, |tx| {
            let row = tx
                .query_typed_opt(
                    "SELECT identity_id FROM credentials
                     WHERE principal_kind = $1 AND kind = $2 AND lookup = $3",
                    &[
                        (&principal.as_str(), Type::TEXT),
                        (&credential.as_str(), Type::TEXT),
                        (&lookup, Type::TEXT),
                    ],
                )
                .await
                .map_err(repo_error)?;
            match row {
                Some(row) => load_identity(tx, col(&row, "identity_id")?).await,
                None => Ok(None),
            }
        })
    }

    async fn insert(&self, identity: &mut Identity) -> Result<(), RepoError> {
        let s = identity.snapshot();
        in_transaction!(self, |tx| insert_identity(tx, &s).await)?;
        identity.set_version(Version::NEW.next());
        Ok(())
    }

    async fn update(&self, identity: &mut Identity) -> Result<(), RepoError> {
        let s = identity.snapshot();
        let version = in_transaction!(self, |tx| update_identity(tx, &s).await)?;
        identity.set_version(version);
        Ok(())
    }
}

async fn load_identity(tx: &impl Queries, id: Uuid) -> Result<Option<Identity>, RepoError> {
    let Some(row) = tx
        .query_typed_opt(
            "SELECT version, principal_kind, principal_id FROM identities WHERE id = $1",
            &[(&id, Type::UUID)],
        )
        .await
        .map_err(repo_error)?
    else {
        return Ok(None);
    };
    let credentials = tx
        .query_typed(
            "SELECT kind, lookup, data::text FROM credentials WHERE identity_id = $1
             ORDER BY kind",
            &[(&id, Type::UUID)],
        )
        .await
        .map_err(repo_error)?
        .iter()
        .map(|c| {
            let data: String = col(c, "data")?;
            Ok(StoredCredential {
                kind: col(c, "kind")?,
                lookup: col(c, "lookup")?,
                data: serde_json::from_str(&data).map_err(corrupt)?,
            })
        })
        .collect::<Result<Vec<_>, RepoError>>()?;
    let principal_kind: String = col(&row, "principal_kind")?;
    let snapshot = IdentitySnapshot {
        id: IdentityId::from_uuid(id),
        version: version_from_db(col(&row, "version")?)?,
        principal: principal_from_db(&principal_kind, col(&row, "principal_id")?)?,
        credentials,
    };
    Identity::restore(snapshot).map(Some).map_err(corrupt)
}

/// `IdentityRepo::insert` inside the caller's transaction (also used by `PgBatch`).
pub(crate) async fn insert_identity(
    tx: &impl Queries,
    s: &IdentitySnapshot,
) -> Result<(), RepoError> {
    let (kind, pid) = principal_to_db(s.principal);
    tx.query_typed(
        "INSERT INTO identities (id, version, principal_kind, principal_id)
         VALUES ($1, 1, $2, $3)",
        &[
            (&s.id.as_uuid(), Type::UUID),
            (&kind, Type::TEXT),
            (&pid, Type::UUID),
        ],
    )
    .await
    .map_err(repo_error)?;
    write_credentials(tx, s).await
}

/// `IdentityRepo::update` inside the caller's transaction; returns the new version.
pub(crate) async fn update_identity(
    tx: &impl Queries,
    s: &IdentitySnapshot,
) -> Result<Version, RepoError> {
    let bumped = tx
        .query_typed_opt(
            "UPDATE identities SET version = version + 1
             WHERE id = $1 AND version = $2 RETURNING version",
            &[
                (&s.id.as_uuid(), Type::UUID),
                (&version_to_db(s.version)?, Type::INT8),
            ],
        )
        .await
        .map_err(repo_error)?;
    let Some(row) = bumped else {
        return Err(RepoError::StaleVersion);
    };
    tx.query_typed(
        "DELETE FROM credentials WHERE identity_id = $1",
        &[(&s.id.as_uuid(), Type::UUID)],
    )
    .await
    .map_err(repo_error)?;
    write_credentials(tx, s).await?;
    version_from_db(col(&row, "version")?)
}

async fn write_credentials(tx: &impl Queries, s: &IdentitySnapshot) -> Result<(), RepoError> {
    let (principal_kind, _) = principal_to_db(s.principal);
    let kinds: Vec<&str> = s.credentials.iter().map(|c| c.kind.as_str()).collect();
    let lookups: Vec<Option<&str>> = s.credentials.iter().map(|c| c.lookup.as_deref()).collect();
    let data: Vec<String> = s.credentials.iter().map(|c| c.data.to_string()).collect();
    tx.query_typed(
        "INSERT INTO credentials (identity_id, principal_kind, kind, lookup, data)
         SELECT $1, $2, k, l, d::jsonb FROM unnest($3::text[], $4::text[], $5::text[]) AS t (k, l, d)",
        &[
            (&s.id.as_uuid(), Type::UUID),
            (&principal_kind, Type::TEXT),
            (&kinds, Type::TEXT_ARRAY),
            (&lookups, Type::TEXT_ARRAY),
            (&data, Type::TEXT_ARRAY),
        ],
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}

// ---------- sessions ----------

const SESSION_PLAIN: &[&str] = &[
    "id",
    "version",
    "identity_id",
    "principal_kind",
    "principal_id",
    "token_hash",
    "state",
    "failed_attempts",
];
const SESSION_TIMESTAMPS: &[&str] = &["mfa_at", "created_at", "expires_at", "revoked_at"];

fn session_columns() -> String {
    prefixed_columns("sessions", "", SESSION_PLAIN, SESSION_TIMESTAMPS)
}

/// A session's columns read from `table` under `prefix`, for a join.
pub(crate) fn session_columns_as(table: &str, prefix: &str) -> String {
    prefixed_columns(table, prefix, SESSION_PLAIN, SESSION_TIMESTAMPS)
}

fn session_from_row(row: &Row) -> Result<Session, RepoError> {
    session_from(Prefixed::plain(row))
}

pub(crate) fn session_from(row: Prefixed<'_>) -> Result<Session, RepoError> {
    let principal_kind: String = row.col("principal_kind")?;
    let state: String = row.col("state")?;
    let failed: i32 = row.col("failed_attempts")?;
    let state = match state.as_str() {
        "pending" => SessionState::Pending {
            failed_attempts: u32::try_from(failed).map_err(corrupt)?,
        },
        "active" => SessionState::Active {
            mfa_at: opt_ts(row.col("mfa_at")?),
        },
        other => return Err(corrupt(format!("unknown session state {other:?}"))),
    };
    Ok(Session::restore(SessionSnapshot {
        id: SessionId::from_uuid(row.col("id")?),
        version: version_from_db(row.col("version")?)?,
        identity: IdentityId::from_uuid(row.col("identity_id")?),
        principal: principal_from_db(&principal_kind, row.col("principal_id")?)?,
        token_hash: token_hash_from_db(row.col("token_hash")?)?,
        state,
        created_at: Timestamp::from_unix_micros(row.col("created_at")?),
        expires_at: Timestamp::from_unix_micros(row.col("expires_at")?),
        revoked_at: opt_ts(row.col("revoked_at")?),
    }))
}

/// The state columns: `(state, failed_attempts, mfa_at)`.
fn state_to_db(state: SessionState) -> (&'static str, i32, Option<i64>) {
    match state {
        SessionState::Pending { failed_attempts } => (
            "pending",
            i32::try_from(failed_attempts).unwrap_or(i32::MAX),
            None,
        ),
        SessionState::Active { mfa_at } => ("active", 0, opt_micros(mfa_at)),
    }
}

async fn select_session(
    client: &impl Queries,
    filter: &str,
    value: &(dyn tokio_postgres::types::ToSql + Sync),
    ty: Type,
) -> Result<Option<Session>, RepoError> {
    client
        .query_typed_opt(
            &format!("SELECT {} FROM sessions WHERE {filter}", session_columns()),
            &[(value, ty)],
        )
        .await
        .map_err(repo_error)?
        .map(|row| session_from_row(&row))
        .transpose()
}

impl<C: PgConnection> SessionRepo for PgStore<C> {
    async fn get(&self, id: SessionId) -> Result<Option<Session>, RepoError> {
        let mut conn = self.conn().await;
        select_session(&conn.db(), "id = $1", &id.as_uuid(), Type::UUID).await
    }

    async fn prune(&self, before: Timestamp, limit: u32) -> Result<u32, RepoError> {
        let before = before.unix_micros();
        in_transaction!(self, |tx| {
            let deleted = tx
                .execute_typed(
                    &format!(
                        "DELETE FROM sessions WHERE id IN (
                             SELECT id FROM sessions
                             WHERE expires_at < {before} OR revoked_at < {before}
                             LIMIT $2
                         )",
                        before = ts(1)
                    ),
                    &[(&before, Type::INT8), (&i64::from(limit), Type::INT8)],
                )
                .await
                .map_err(repo_error)?;
            u32::try_from(deleted).map_err(corrupt)
        })
    }

    async fn get_by_token(&self, hash: TokenHash) -> Result<Option<Session>, RepoError> {
        let mut conn = self.conn().await;
        let bytes = hash.as_bytes().as_slice();
        select_session(&conn.db(), "token_hash = $1", &bytes, Type::BYTEA).await
    }

    async fn insert(&self, session: &mut Session) -> Result<(), RepoError> {
        let s = session.snapshot();
        in_transaction!(self, |tx| insert_session(tx, &s).await)?;
        session.set_version(Version::NEW.next());
        Ok(())
    }

    async fn update(&self, session: &mut Session) -> Result<(), RepoError> {
        let s = session.snapshot();
        let version = in_transaction!(self, |tx| update_session(tx, &s).await)?;
        session.set_version(version);
        Ok(())
    }
}

pub(crate) async fn insert_session(
    client: &impl Queries,
    s: &SessionSnapshot,
) -> Result<(), RepoError> {
    let (kind, pid) = principal_to_db(s.principal);
    let (state, failed, mfa_at) = state_to_db(s.state);
    client
        .query_typed(
            &format!(
                "INSERT INTO sessions (id, version, identity_id, principal_kind, principal_id,
                     token_hash, state, failed_attempts, mfa_at, created_at, expires_at,
                     revoked_at)
                 VALUES ($1, 1, $2, $3, $4, $5, $6, $7, {}, {}, {}, {})",
                ts(8),
                ts(9),
                ts(10),
                ts(11)
            ),
            &[
                (&s.id.as_uuid(), Type::UUID),
                (&s.identity.as_uuid(), Type::UUID),
                (&kind, Type::TEXT),
                (&pid, Type::UUID),
                (&s.token_hash.as_bytes().as_slice(), Type::BYTEA),
                (&state, Type::TEXT),
                (&failed, Type::INT4),
                (&mfa_at, Type::INT8),
                (&s.created_at.unix_micros(), Type::INT8),
                (&s.expires_at.unix_micros(), Type::INT8),
                (&opt_micros(s.revoked_at), Type::INT8),
            ],
        )
        .await
        .map_err(repo_error)?;
    Ok(())
}

pub(crate) async fn update_session(
    client: &impl Queries,
    s: &SessionSnapshot,
) -> Result<Version, RepoError> {
    let (state, failed, mfa_at) = state_to_db(s.state);
    let row = client
        .query_typed_opt(
            &format!(
                "UPDATE sessions SET version = version + 1, token_hash = $3, state = $4,
                     failed_attempts = $5, mfa_at = {}, expires_at = {}, revoked_at = {}
                 WHERE id = $1 AND version = $2 RETURNING version",
                ts(6),
                ts(7),
                ts(8)
            ),
            &[
                (&s.id.as_uuid(), Type::UUID),
                (&version_to_db(s.version)?, Type::INT8),
                (&s.token_hash.as_bytes().as_slice(), Type::BYTEA),
                (&state, Type::TEXT),
                (&failed, Type::INT4),
                (&mfa_at, Type::INT8),
                (&s.expires_at.unix_micros(), Type::INT8),
                (&opt_micros(s.revoked_at), Type::INT8),
            ],
        )
        .await
        .map_err(repo_error)?;
    let Some(row) = row else {
        return Err(RepoError::StaleVersion);
    };
    version_from_db(col(&row, "version")?)
}

/// `StageSessions::revoke_sessions` inside the batch's transaction.
pub(crate) async fn revoke_sessions(
    client: &impl Queries,
    identity: IdentityId,
    keep: Option<SessionId>,
    at: Timestamp,
) -> Result<(), RepoError> {
    client
        .query_typed(
            &format!(
                "UPDATE sessions SET version = version + 1, revoked_at = {}
                 WHERE identity_id = $1 AND revoked_at IS NULL
                   AND id IS DISTINCT FROM $2",
                ts(3)
            ),
            &[
                (&identity.as_uuid(), Type::UUID),
                (&keep.map(|k| k.as_uuid()), Type::UUID),
                (&at.unix_micros(), Type::INT8),
            ],
        )
        .await
        .map_err(repo_error)?;
    Ok(())
}

// ---------- API keys ----------

fn key_columns() -> String {
    format!(
        "id, version, kind, name, token_hash, last_four, scopes, channel_id, created_by_kind,
         created_by_id, {}, {}",
        micros("created_at"),
        micros("revoked_at")
    )
}

fn key_from_row(row: &Row) -> Result<ApiKey, RepoError> {
    let kind: String = col(row, "kind")?;
    let created_by_kind: String = col(row, "created_by_kind")?;
    let snapshot = ApiKeySnapshot {
        id: ApiKeyId::from_uuid(col(row, "id")?),
        version: version_from_db(col(row, "version")?)?,
        kind: ApiKeyKind::parse(&kind)
            .ok_or_else(|| corrupt(format!("unknown api key kind {kind:?}")))?,
        name: col(row, "name")?,
        token_hash: token_hash_from_db(col(row, "token_hash")?)?,
        last_four: col(row, "last_four")?,
        scopes: scopes_from_db(col(row, "scopes")?)?,
        channel: col::<Option<uuid::Uuid>>(row, "channel_id")?.map(ChannelId::from_uuid),
        created_by: actor_from_db(&created_by_kind, col(row, "created_by_id")?)?,
        created_at: Timestamp::from_unix_micros(col(row, "created_at")?),
        revoked_at: opt_ts(col(row, "revoked_at")?),
    };
    ApiKey::restore(snapshot).map_err(corrupt)
}

impl<C: PgConnection> ApiKeyRepo for PgStore<C> {
    async fn get(&self, id: ApiKeyId) -> Result<Option<ApiKey>, RepoError> {
        let mut conn = self.conn().await;
        conn.db()
            .query_typed_opt(
                &format!("SELECT {} FROM api_keys WHERE id = $1", key_columns()),
                &[(&id.as_uuid(), Type::UUID)],
            )
            .await
            .map_err(repo_error)?
            .map(|row| key_from_row(&row))
            .transpose()
    }

    async fn get_by_token(&self, hash: TokenHash) -> Result<Option<ApiKey>, RepoError> {
        let mut conn = self.conn().await;
        conn.db()
            .query_typed_opt(
                &format!(
                    "SELECT {} FROM api_keys WHERE token_hash = $1",
                    key_columns()
                ),
                &[(&hash.as_bytes().as_slice(), Type::BYTEA)],
            )
            .await
            .map_err(repo_error)?
            .map(|row| key_from_row(&row))
            .transpose()
    }

    async fn list(
        &self,
        page: hypha_core::PageRequest<ApiKeyId>,
    ) -> Result<hypha_core::Page<ApiKey>, RepoError> {
        let after = page.after.map(|id| id.as_uuid());
        let fetch = i64::from(page.limit.get()) + 1;
        let mut conn = self.conn().await;
        let keys = conn
            .db()
            .query_typed(
                &format!(
                    "SELECT {} FROM api_keys WHERE ($1::uuid IS NULL OR id < $1)
                     ORDER BY id DESC LIMIT $2",
                    key_columns()
                ),
                &[(&after, Type::UUID), (&fetch, Type::INT8)],
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(key_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(hypha_core::Page::from_overfetch(keys, page.limit))
    }

    async fn insert(&self, key: &mut ApiKey) -> Result<(), RepoError> {
        let s = key.snapshot();
        in_transaction!(self, |tx| insert_api_key(tx, &s).await)?;
        key.set_version(Version::NEW.next());
        Ok(())
    }

    async fn update(&self, key: &mut ApiKey) -> Result<(), RepoError> {
        let s = key.snapshot();
        let version = in_transaction!(self, |tx| update_api_key(tx, &s).await)?;
        key.set_version(version);
        Ok(())
    }
}

/// `ApiKeyRepo::insert` inside the caller's transaction.
pub(crate) async fn insert_api_key(tx: &impl Queries, s: &ApiKeySnapshot) -> Result<(), RepoError> {
    let (by_kind, by_id) = actor_to_db(s.created_by);
    tx.query_typed(
        &format!(
            "INSERT INTO api_keys (id, version, kind, name, token_hash, last_four,
                 scopes, created_by_kind, created_by_id, created_at, revoked_at, channel_id)
             VALUES ($1, 1, $2, $3, $4, $5, $6, $7, $8, {}, {}, $11)",
            ts(9),
            ts(10)
        ),
        &[
            (&s.id.as_uuid(), Type::UUID),
            (&s.kind.as_str(), Type::TEXT),
            (&s.name, Type::TEXT),
            (&s.token_hash.as_bytes().as_slice(), Type::BYTEA),
            (&s.last_four, Type::TEXT),
            (&scopes_to_db(&s.scopes), Type::TEXT_ARRAY),
            (&by_kind, Type::TEXT),
            (&by_id, Type::UUID),
            (&s.created_at.unix_micros(), Type::INT8),
            (&opt_micros(s.revoked_at), Type::INT8),
            (&s.channel.map(|c| c.as_uuid()), Type::UUID),
        ],
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}

/// `ApiKeyRepo::update` inside the caller's transaction; the new version.
pub(crate) async fn update_api_key(
    tx: &impl Queries,
    s: &ApiKeySnapshot,
) -> Result<Version, RepoError> {
    let row = tx
        .query_typed_opt(
            &format!(
                "UPDATE api_keys SET version = version + 1, name = $3, scopes = $4,
                     revoked_at = {}
                 WHERE id = $1 AND version = $2 RETURNING version",
                ts(5)
            ),
            &[
                (&s.id.as_uuid(), Type::UUID),
                (&version_to_db(s.version)?, Type::INT8),
                (&s.name, Type::TEXT),
                (&scopes_to_db(&s.scopes), Type::TEXT_ARRAY),
                (&opt_micros(s.revoked_at), Type::INT8),
            ],
        )
        .await
        .map_err(repo_error)?;
    let row = row.ok_or(RepoError::StaleVersion)?;
    version_from_db(col(&row, "version")?)
}

// ---------- password resets ----------

fn reset_columns() -> String {
    format!(
        "id, version, identity_id, token_hash, {}, {}, {}",
        micros("created_at"),
        micros("expires_at"),
        micros("used_at")
    )
}

fn reset_from_row(row: &Row) -> Result<PasswordReset, RepoError> {
    Ok(PasswordReset::restore(PasswordResetSnapshot {
        id: PasswordResetId::from_uuid(col(row, "id")?),
        version: version_from_db(col(row, "version")?)?,
        identity: IdentityId::from_uuid(col(row, "identity_id")?),
        token_hash: token_hash_from_db(col(row, "token_hash")?)?,
        created_at: Timestamp::from_unix_micros(col(row, "created_at")?),
        expires_at: Timestamp::from_unix_micros(col(row, "expires_at")?),
        used_at: opt_ts(col(row, "used_at")?),
    }))
}

impl<C: PgConnection> PasswordResetRepo for PgStore<C> {
    async fn get_by_token(&self, hash: TokenHash) -> Result<Option<PasswordReset>, RepoError> {
        let mut conn = self.conn().await;
        conn.db()
            .query_typed_opt(
                &format!(
                    "SELECT {} FROM password_resets WHERE token_hash = $1",
                    reset_columns()
                ),
                &[(&hash.as_bytes().as_slice(), Type::BYTEA)],
            )
            .await
            .map_err(repo_error)?
            .map(|row| reset_from_row(&row))
            .transpose()
    }

    async fn latest_for_identity(
        &self,
        identity: IdentityId,
    ) -> Result<Option<PasswordReset>, RepoError> {
        let mut conn = self.conn().await;
        conn.db()
            .query_typed_opt(
                &format!(
                    "SELECT {} FROM password_resets WHERE identity_id = $1
                     ORDER BY created_at DESC, id DESC LIMIT 1",
                    reset_columns()
                ),
                &[(&identity.as_uuid(), Type::UUID)],
            )
            .await
            .map_err(repo_error)?
            .map(|row| reset_from_row(&row))
            .transpose()
    }

    async fn insert(&self, reset: &mut PasswordReset) -> Result<(), RepoError> {
        let s = reset.snapshot();
        in_transaction!(self, |tx| insert_password_reset(tx, &s).await)?;
        reset.set_version(Version::NEW.next());
        Ok(())
    }
}

/// A reset's update, inside the batch's transaction (resets change only in batches).
pub(crate) async fn update_password_reset(
    client: &impl Queries,
    s: &PasswordResetSnapshot,
) -> Result<(), RepoError> {
    let row = client
        .query_typed_opt(
            &format!(
                "UPDATE password_resets SET version = version + 1, used_at = {}
                 WHERE id = $1 AND version = $2 RETURNING version",
                ts(3)
            ),
            &[
                (&s.id.as_uuid(), Type::UUID),
                (&version_to_db(s.version)?, Type::INT8),
                (&opt_micros(s.used_at), Type::INT8),
            ],
        )
        .await
        .map_err(repo_error)?;
    row.map(|_| ()).ok_or(RepoError::StaleVersion)
}

/// `StagePasswordResets::use_up_password_resets` inside the batch's transaction.
pub(crate) async fn use_up_password_resets(
    client: &impl Queries,
    identity: IdentityId,
    at: Timestamp,
) -> Result<(), RepoError> {
    client
        .query_typed(
            &format!(
                "UPDATE password_resets SET version = version + 1, used_at = {}
                 WHERE identity_id = $1 AND used_at IS NULL",
                ts(2)
            ),
            &[
                (&identity.as_uuid(), Type::UUID),
                (&at.unix_micros(), Type::INT8),
            ],
        )
        .await
        .map_err(repo_error)?;
    Ok(())
}

/// `PasswordResetRepo::insert` inside the caller's transaction (also used by `PgBatch`).
pub(crate) async fn insert_password_reset(
    client: &impl Queries,
    s: &PasswordResetSnapshot,
) -> Result<(), RepoError> {
    client
        .query_typed(
            &format!(
                "INSERT INTO password_resets (id, version, identity_id, token_hash,
                     created_at, expires_at, used_at)
                 VALUES ($1, 1, $2, $3, {}, {}, {})",
                ts(4),
                ts(5),
                ts(6)
            ),
            &[
                (&s.id.as_uuid(), Type::UUID),
                (&s.identity.as_uuid(), Type::UUID),
                (&s.token_hash.as_bytes().as_slice(), Type::BYTEA),
                (&s.created_at.unix_micros(), Type::INT8),
                (&s.expires_at.unix_micros(), Type::INT8),
                (&opt_micros(s.used_at), Type::INT8),
            ],
        )
        .await
        .map_err(repo_error)?;
    Ok(())
}

/// A channel exists for a new publishable key (design note 0020): `hypha-auth` asks
/// through its port, the channels table answers.
impl<C: PgConnection> hypha_auth::ChannelDirectory for PgStore<C> {
    async fn channel_exists(&self, channel: ChannelId) -> Result<bool, RepoError> {
        Ok(hypha_channels::ChannelRepo::get(self, channel)
            .await?
            .is_some())
    }
}
