//! OAuth's requests, codes and token pairs (design note 0029, §15).

use hypha_auth::TokenHash;
use hypha_core::{AppKey, OAuthRequestId, RepoError, Timestamp, Version, Versioned};
use hypha_oauth::{
    AuthorizationCode, AuthorizationRequest, CodeSnapshot, OAuthCodeRepo, OAuthRequestRepo,
    OAuthToken, OAuthTokenRepo, RequestSnapshot, RequestStatus, TokenSnapshot,
};
use tokio_postgres::{Row, types::Type};
use uuid::Uuid;

use crate::{
    columns::{
        actor_from_db, actor_to_db, col, micros, opt_micros, opt_ts, scopes_from_db, scopes_to_db,
        token_hash_from_db, ts, version_from_db, version_to_db,
    },
    error::{corrupt, repo_error},
    store::{PgConnection, PgStore, Queries, in_transaction},
};

// ---------- requests ----------

fn request_columns() -> String {
    format!(
        "id, version, app_key, client_id, manifest::text AS manifest, redirect_uri, state,
         code_challenge, scopes, previous_scopes, status, decided_by_kind, decided_by_id, {}, {}, {}",
        micros("decided_at"),
        micros("created_at"),
        micros("expires_at")
    )
}

fn request_from_row(row: &Row) -> Result<AuthorizationRequest, RepoError> {
    let id = OAuthRequestId::from_uuid(col(row, "id")?);
    let manifest: String = col(row, "manifest")?;
    let status: String = col(row, "status")?;
    let decided_by = match col::<Option<String>>(row, "decided_by_kind")? {
        Some(kind) => Some(actor_from_db(&kind, col(row, "decided_by_id")?)?),
        None => None,
    };
    let previous: Option<Vec<String>> = col(row, "previous_scopes")?;
    AuthorizationRequest::restore(RequestSnapshot {
        id,
        version: version_from_db(col(row, "version")?)?,
        app: col(row, "app_key")?,
        client_id: col(row, "client_id")?,
        manifest: serde_json::from_str(&manifest)
            .map_err(|e| corrupt(format!("request {id} manifest: {e}")))?,
        redirect_uri: col(row, "redirect_uri")?,
        state: col(row, "state")?,
        code_challenge: col(row, "code_challenge")?,
        scopes: scopes_from_db(col(row, "scopes")?)?,
        previous_scopes: previous.map(scopes_from_db).transpose()?,
        status: RequestStatus::parse(&status)
            .ok_or_else(|| corrupt(format!("request {id}: status {status:?}")))?,
        decided_by,
        decided_at: opt_ts(col(row, "decided_at")?),
        created_at: Timestamp::from_unix_micros(col(row, "created_at")?),
        expires_at: Timestamp::from_unix_micros(col(row, "expires_at")?),
    })
    .map_err(|e| corrupt(format!("request {id}: {e}")))
}

pub(crate) async fn insert_request(
    tx: &impl Queries,
    s: &RequestSnapshot,
) -> Result<(), RepoError> {
    let decided = s.decided_by.map(actor_to_db);
    tx.query_typed(
        &format!(
            "INSERT INTO oauth_requests (id, version, app_key, client_id, manifest, redirect_uri,
                 state, code_challenge, scopes, previous_scopes, status, decided_by_kind,
                 decided_by_id, decided_at, created_at, expires_at)
             VALUES ($1, 1, $2, $3, $4::text::jsonb, $5, $6, $7, $8, $9, $10, $11, $12, {}, {}, {})",
            ts(13),
            ts(14),
            ts(15)
        ),
        &[
            (&s.id.as_uuid(), Type::UUID),
            (&s.app, Type::TEXT),
            (&s.client_id, Type::TEXT),
            (&s.manifest.to_string(), Type::TEXT),
            (&s.redirect_uri, Type::TEXT),
            (&s.state, Type::TEXT),
            (&s.code_challenge, Type::TEXT),
            (&scopes_to_db(&s.scopes), Type::TEXT_ARRAY),
            (
                &s.previous_scopes.as_deref().map(scopes_to_db),
                Type::TEXT_ARRAY,
            ),
            (&s.status.as_str(), Type::TEXT),
            (&decided.map(|d| d.0), Type::TEXT),
            (&decided.map(|d| d.1), Type::UUID),
            (&opt_micros(s.decided_at), Type::INT8),
            (&s.created_at.unix_micros(), Type::INT8),
            (&s.expires_at.unix_micros(), Type::INT8),
        ],
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}

pub(crate) async fn update_request(
    tx: &impl Queries,
    s: &RequestSnapshot,
) -> Result<(), RepoError> {
    let decided = s.decided_by.map(actor_to_db);
    tx.query_typed_opt(
        &format!(
            "UPDATE oauth_requests SET version = version + 1, status = $3, decided_by_kind = $4,
                 decided_by_id = $5, decided_at = {}
             WHERE id = $1 AND version = $2 RETURNING version",
            ts(6)
        ),
        &[
            (&s.id.as_uuid(), Type::UUID),
            (&version_to_db(s.version)?, Type::INT8),
            (&s.status.as_str(), Type::TEXT),
            (&decided.map(|d| d.0), Type::TEXT),
            (&decided.map(|d| d.1), Type::UUID),
            (&opt_micros(s.decided_at), Type::INT8),
        ],
    )
    .await
    .map_err(repo_error)?
    .ok_or(RepoError::StaleVersion)?;
    Ok(())
}

impl<C: PgConnection> OAuthRequestRepo for PgStore<C> {
    async fn get(&self, id: OAuthRequestId) -> Result<Option<AuthorizationRequest>, RepoError> {
        let mut conn = self.conn().await;
        conn.db()
            .query_typed_opt(
                &format!(
                    "SELECT {} FROM oauth_requests WHERE id = $1",
                    request_columns()
                ),
                &[(&id.as_uuid(), Type::UUID)],
            )
            .await
            .map_err(repo_error)?
            .as_ref()
            .map(request_from_row)
            .transpose()
    }

    async fn list_for_app(
        &self,
        app: &AppKey,
        limit: u32,
    ) -> Result<Vec<AuthorizationRequest>, RepoError> {
        let mut conn = self.conn().await;
        conn.db()
            .query_typed(
                &format!(
                    "SELECT {} FROM oauth_requests WHERE app_key = $1
                     ORDER BY created_at DESC, id DESC LIMIT $2",
                    request_columns()
                ),
                &[(&app.as_str(), Type::TEXT), (&i64::from(limit), Type::INT8)],
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(request_from_row)
            .collect()
    }

    async fn insert(&self, request: &mut AuthorizationRequest) -> Result<(), RepoError> {
        let s = request.snapshot();
        in_transaction!(self, |tx| insert_request(tx, &s).await)?;
        request.set_version(Version::NEW.next());
        Ok(())
    }

    async fn update(&self, request: &mut AuthorizationRequest) -> Result<(), RepoError> {
        let s = request.snapshot();
        in_transaction!(self, |tx| update_request(tx, &s).await)?;
        request.set_version(request.version().next());
        Ok(())
    }
}

// ---------- codes ----------

fn code_columns() -> String {
    format!(
        "hash, version, request_id, app_key, client_id, redirect_uri, code_challenge, scopes,
         first_install, family, {}, {}",
        micros("expires_at"),
        micros("used_at")
    )
}

fn code_from_row(row: &Row) -> Result<AuthorizationCode, RepoError> {
    AuthorizationCode::restore(CodeSnapshot {
        hash: token_hash_from_db(col(row, "hash")?)?,
        version: version_from_db(col(row, "version")?)?,
        request: OAuthRequestId::from_uuid(col(row, "request_id")?),
        app: col(row, "app_key")?,
        client_id: col(row, "client_id")?,
        redirect_uri: col(row, "redirect_uri")?,
        code_challenge: col(row, "code_challenge")?,
        scopes: scopes_from_db(col(row, "scopes")?)?,
        first_install: col(row, "first_install")?,
        family: col(row, "family")?,
        expires_at: Timestamp::from_unix_micros(col(row, "expires_at")?),
        used_at: opt_ts(col(row, "used_at")?),
    })
    .map_err(corrupt)
}

pub(crate) async fn insert_code(tx: &impl Queries, s: &CodeSnapshot) -> Result<(), RepoError> {
    tx.query_typed(
        &format!(
            "INSERT INTO oauth_codes (hash, version, request_id, app_key, client_id,
                 redirect_uri, code_challenge, scopes, first_install, family, expires_at, used_at)
             VALUES ($1, 1, $2, $3, $4, $5, $6, $7, $8, $9, {}, {})",
            ts(10),
            ts(11)
        ),
        &[
            (&s.hash.as_bytes().as_slice(), Type::BYTEA),
            (&s.request.as_uuid(), Type::UUID),
            (&s.app, Type::TEXT),
            (&s.client_id, Type::TEXT),
            (&s.redirect_uri, Type::TEXT),
            (&s.code_challenge, Type::TEXT),
            (&scopes_to_db(&s.scopes), Type::TEXT_ARRAY),
            (&s.first_install, Type::BOOL),
            (&s.family, Type::UUID),
            (&s.expires_at.unix_micros(), Type::INT8),
            (&opt_micros(s.used_at), Type::INT8),
        ],
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}

pub(crate) async fn update_code(tx: &impl Queries, s: &CodeSnapshot) -> Result<(), RepoError> {
    tx.query_typed_opt(
        &format!(
            "UPDATE oauth_codes SET version = version + 1, used_at = {}
             WHERE hash = $1 AND version = $2 RETURNING version",
            ts(3)
        ),
        &[
            (&s.hash.as_bytes().as_slice(), Type::BYTEA),
            (&version_to_db(s.version)?, Type::INT8),
            (&opt_micros(s.used_at), Type::INT8),
        ],
    )
    .await
    .map_err(repo_error)?
    .ok_or(RepoError::StaleVersion)?;
    Ok(())
}

impl<C: PgConnection> OAuthCodeRepo for PgStore<C> {
    async fn get(&self, hash: TokenHash) -> Result<Option<AuthorizationCode>, RepoError> {
        let mut conn = self.conn().await;
        conn.db()
            .query_typed_opt(
                &format!("SELECT {} FROM oauth_codes WHERE hash = $1", code_columns()),
                &[(&hash.as_bytes().as_slice(), Type::BYTEA)],
            )
            .await
            .map_err(repo_error)?
            .as_ref()
            .map(code_from_row)
            .transpose()
    }

    async fn insert(&self, code: &mut AuthorizationCode) -> Result<(), RepoError> {
        let s = code.snapshot();
        in_transaction!(self, |tx| insert_code(tx, &s).await)?;
        code.set_version(Version::NEW.next());
        Ok(())
    }

    async fn update(&self, code: &mut AuthorizationCode) -> Result<(), RepoError> {
        let s = code.snapshot();
        in_transaction!(self, |tx| update_code(tx, &s).await)?;
        code.set_version(code.version().next());
        Ok(())
    }
}

// ---------- tokens ----------

fn token_columns() -> String {
    format!(
        "id, version, family, app_key, scopes, access_hash, refresh_hash, {}, {}, {}, {}, {}",
        micros("access_expires_at"),
        micros("refresh_expires_at"),
        micros("created_at"),
        micros("rotated_at"),
        micros("revoked_at")
    )
}

fn token_from_row(row: &Row) -> Result<OAuthToken, RepoError> {
    OAuthToken::restore(TokenSnapshot {
        id: col(row, "id")?,
        version: version_from_db(col(row, "version")?)?,
        family: col(row, "family")?,
        app: col(row, "app_key")?,
        scopes: scopes_from_db(col(row, "scopes")?)?,
        access_hash: token_hash_from_db(col(row, "access_hash")?)?,
        access_expires_at: Timestamp::from_unix_micros(col(row, "access_expires_at")?),
        refresh_hash: token_hash_from_db(col(row, "refresh_hash")?)?,
        refresh_expires_at: Timestamp::from_unix_micros(col(row, "refresh_expires_at")?),
        created_at: Timestamp::from_unix_micros(col(row, "created_at")?),
        rotated_at: opt_ts(col(row, "rotated_at")?),
        revoked_at: opt_ts(col(row, "revoked_at")?),
    })
    .map_err(corrupt)
}

pub(crate) async fn insert_token(tx: &impl Queries, s: &TokenSnapshot) -> Result<(), RepoError> {
    tx.query_typed(
        &format!(
            "INSERT INTO oauth_refresh_tokens (id, version, family, app_key, scopes, access_hash,
                 access_expires_at, refresh_hash, refresh_expires_at, created_at, rotated_at,
                 revoked_at)
             VALUES ($1, 1, $2, $3, $4, $5, {}, $7, {}, {}, {}, {})",
            ts(6),
            ts(8),
            ts(9),
            ts(10),
            ts(11)
        ),
        &[
            (&s.id, Type::UUID),
            (&s.family, Type::UUID),
            (&s.app, Type::TEXT),
            (&scopes_to_db(&s.scopes), Type::TEXT_ARRAY),
            (&s.access_hash.as_bytes().as_slice(), Type::BYTEA),
            (&s.access_expires_at.unix_micros(), Type::INT8),
            (&s.refresh_hash.as_bytes().as_slice(), Type::BYTEA),
            (&s.refresh_expires_at.unix_micros(), Type::INT8),
            (&s.created_at.unix_micros(), Type::INT8),
            (&opt_micros(s.rotated_at), Type::INT8),
            (&opt_micros(s.revoked_at), Type::INT8),
        ],
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}

pub(crate) async fn update_token(tx: &impl Queries, s: &TokenSnapshot) -> Result<(), RepoError> {
    tx.query_typed_opt(
        &format!(
            "UPDATE oauth_refresh_tokens SET version = version + 1, rotated_at = {},
                 revoked_at = {}
             WHERE id = $1 AND version = $2 RETURNING version",
            ts(3),
            ts(4)
        ),
        &[
            (&s.id, Type::UUID),
            (&version_to_db(s.version)?, Type::INT8),
            (&opt_micros(s.rotated_at), Type::INT8),
            (&opt_micros(s.revoked_at), Type::INT8),
        ],
    )
    .await
    .map_err(repo_error)?
    .ok_or(RepoError::StaleVersion)?;
    Ok(())
}

/// Revokes the app's live tokens at `at` in the caller's transaction; how many.
pub(crate) async fn revoke_app_tokens(
    tx: &impl Queries,
    app: &str,
    at: Timestamp,
) -> Result<u32, RepoError> {
    let rows = tx
        .query_typed(
            &format!(
                "UPDATE oauth_refresh_tokens SET version = version + 1, revoked_at = {}
                 WHERE app_key = $1 AND revoked_at IS NULL RETURNING id",
                ts(2)
            ),
            &[(&app, Type::TEXT), (&at.unix_micros(), Type::INT8)],
        )
        .await
        .map_err(repo_error)?;
    u32::try_from(rows.len()).map_err(corrupt)
}

impl<C: PgConnection> OAuthTokenRepo for PgStore<C> {
    async fn get_by_access(&self, hash: TokenHash) -> Result<Option<OAuthToken>, RepoError> {
        let mut conn = self.conn().await;
        conn.db()
            .query_typed_opt(
                &format!(
                    "SELECT {} FROM oauth_refresh_tokens WHERE access_hash = $1",
                    token_columns()
                ),
                &[(&hash.as_bytes().as_slice(), Type::BYTEA)],
            )
            .await
            .map_err(repo_error)?
            .as_ref()
            .map(token_from_row)
            .transpose()
    }

    async fn get_by_refresh(&self, hash: TokenHash) -> Result<Option<OAuthToken>, RepoError> {
        let mut conn = self.conn().await;
        conn.db()
            .query_typed_opt(
                &format!(
                    "SELECT {} FROM oauth_refresh_tokens WHERE refresh_hash = $1",
                    token_columns()
                ),
                &[(&hash.as_bytes().as_slice(), Type::BYTEA)],
            )
            .await
            .map_err(repo_error)?
            .as_ref()
            .map(token_from_row)
            .transpose()
    }

    async fn insert(&self, token: &mut OAuthToken) -> Result<(), RepoError> {
        let s = token.snapshot();
        in_transaction!(self, |tx| insert_token(tx, &s).await)?;
        token.set_version(Version::NEW.next());
        Ok(())
    }

    async fn update(&self, token: &mut OAuthToken) -> Result<(), RepoError> {
        let s = token.snapshot();
        in_transaction!(self, |tx| update_token(tx, &s).await)?;
        token.set_version(token.version().next());
        Ok(())
    }

    async fn revoke_family(&self, family: Uuid, at: Timestamp) -> Result<u32, RepoError> {
        in_transaction!(self, |tx| {
            let rows = tx
                .query_typed(
                    &format!(
                        "UPDATE oauth_refresh_tokens SET version = version + 1, revoked_at = {}
                         WHERE family = $1 AND revoked_at IS NULL RETURNING id",
                        ts(2)
                    ),
                    &[(&family, Type::UUID), (&at.unix_micros(), Type::INT8)],
                )
                .await
                .map_err(repo_error)?;
            u32::try_from(rows.len()).map_err(corrupt)
        })
    }

    async fn revoke_app(&self, app: &AppKey, at: Timestamp) -> Result<u32, RepoError> {
        let app = app.as_str().to_string();
        in_transaction!(self, |tx| revoke_app_tokens(tx, &app, at).await)
    }
}
