//! `hypha-users`'s repositories: users, roles and invites (design note 0008).

use hypha_auth::TokenHash;
use hypha_core::{Email, InviteId, RepoError, RoleId, Timestamp, UserId, Version, Versioned};
use hypha_users::{
    Invite, InviteRepo, InviteSnapshot, Role, RoleRepo, RoleScopes, RoleSnapshot,
    StaffSessionRecord, StaffSessionRepo, User, UserRepo, UserSnapshot, UserStatus,
};
use tokio_postgres::{Row, types::Type};
use uuid::Uuid;

use crate::{
    auth::{session_columns_as, session_from},
    columns::{
        Prefixed, actor_from_db, actor_to_db, col, micros, prefixed_columns, scopes_from_db,
        scopes_to_db, token_hash_from_db, ts, version_from_db, version_to_db,
    },
    error::{corrupt, repo_error},
    store::Queries,
    store::{PgConnection, PgStore, in_transaction},
};

// ---------- users ----------

const USER_PLAIN: &[&str] = &["id", "version", "email", "name", "role_id", "status"];
const USER_TIMESTAMPS: &[&str] = &["created_at"];

fn user_columns() -> String {
    prefixed_columns("users", "", USER_PLAIN, USER_TIMESTAMPS)
}

fn user_from_row(row: &Row) -> Result<User, RepoError> {
    user_from(Prefixed::plain(row))
}

fn user_from(row: Prefixed<'_>) -> Result<User, RepoError> {
    let email: String = row.col("email")?;
    let status: String = row.col("status")?;
    User::restore(UserSnapshot {
        id: UserId::from_uuid(row.col("id")?),
        version: version_from_db(row.col("version")?)?,
        email: Email::new(&email).map_err(corrupt)?,
        name: row.col("name")?,
        role: RoleId::from_uuid(row.col("role_id")?),
        status: UserStatus::parse(&status)
            .ok_or_else(|| corrupt(format!("unknown user status {status:?}")))?,
        created_at: Timestamp::from_unix_micros(row.col("created_at")?),
    })
    .map_err(corrupt)
}

impl<C: PgConnection> UserRepo for PgStore<C> {
    async fn get(&self, id: UserId) -> Result<Option<User>, RepoError> {
        let mut conn = self.conn().await;
        conn.db()
            .query_typed_opt(
                &format!("SELECT {} FROM users WHERE id = $1", user_columns()),
                &[(&id.as_uuid(), Type::UUID)],
            )
            .await
            .map_err(repo_error)?
            .map(|row| user_from_row(&row))
            .transpose()
    }

    async fn get_by_email(&self, email: &Email) -> Result<Option<User>, RepoError> {
        let mut conn = self.conn().await;
        conn.db()
            .query_typed_opt(
                &format!("SELECT {} FROM users WHERE email = $1", user_columns()),
                &[(&email.as_str(), Type::TEXT)],
            )
            .await
            .map_err(repo_error)?
            .map(|row| user_from_row(&row))
            .transpose()
    }

    async fn list_all(&self) -> Result<Vec<User>, RepoError> {
        let mut conn = self.conn().await;
        conn.db()
            .query_typed(
                &format!("SELECT {} FROM users ORDER BY id", user_columns()),
                &[],
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(user_from_row)
            .collect()
    }

    async fn any(&self) -> Result<bool, RepoError> {
        let mut conn = self.conn().await;
        let row = conn
            .db()
            .query_typed_one("SELECT EXISTS (SELECT 1 FROM users) AS found", &[])
            .await
            .map_err(repo_error)?;
        col(&row, "found")
    }

    async fn count_active_with_role(&self, role: RoleId) -> Result<usize, RepoError> {
        let mut conn = self.conn().await;
        let row = conn
            .db()
            .query_typed_one(
                "SELECT count(*) AS count FROM users WHERE role_id = $1 AND status = 'active'",
                &[(&role.as_uuid(), Type::UUID)],
            )
            .await
            .map_err(repo_error)?;
        let count: i64 = col(&row, "count")?;
        usize::try_from(count).map_err(corrupt)
    }

    async fn insert(&self, user: &mut User) -> Result<(), RepoError> {
        let s = user.snapshot();
        in_transaction!(self, |tx| insert_user(tx, &s).await)?;
        user.set_version(Version::NEW.next());
        Ok(())
    }

    async fn update(&self, user: &mut User) -> Result<(), RepoError> {
        let s = user.snapshot();
        let version = in_transaction!(self, |tx| update_user(tx, &s).await)?;
        user.set_version(version);
        Ok(())
    }
}

pub(crate) async fn update_user(
    client: &impl Queries,
    s: &UserSnapshot,
) -> Result<Version, RepoError> {
    let row = client
        .query_typed_opt(
            "UPDATE users SET version = version + 1, email = $3, name = $4, role_id = $5,
                 status = $6
             WHERE id = $1 AND version = $2 RETURNING version",
            &[
                (&s.id.as_uuid(), Type::UUID),
                (&version_to_db(s.version)?, Type::INT8),
                (&s.email.as_str(), Type::TEXT),
                (&s.name, Type::TEXT),
                (&s.role.as_uuid(), Type::UUID),
                (&s.status.as_str(), Type::TEXT),
            ],
        )
        .await
        .map_err(repo_error)?;
    let Some(row) = row else {
        return Err(RepoError::StaleVersion);
    };
    version_from_db(col(&row, "version")?)
}

pub(crate) async fn insert_user(client: &impl Queries, s: &UserSnapshot) -> Result<(), RepoError> {
    client
        .query_typed(
            &format!(
                "INSERT INTO users (id, version, email, name, role_id, status, created_at)
                 VALUES ($1, 1, $2, $3, $4, $5, {})",
                ts(6)
            ),
            &[
                (&s.id.as_uuid(), Type::UUID),
                (&s.email.as_str(), Type::TEXT),
                (&s.name, Type::TEXT),
                (&s.role.as_uuid(), Type::UUID),
                (&s.status.as_str(), Type::TEXT),
                (&s.created_at.unix_micros(), Type::INT8),
            ],
        )
        .await
        .map_err(repo_error)?;
    Ok(())
}

// ---------- staff sessions ----------

/// One query: the session by token, its user and their role, left-joined so a missing
/// user or role comes back as `None` for `authenticate_staff` to judge.
impl<C: PgConnection> StaffSessionRepo for PgStore<C> {
    async fn staff_session_by_token(
        &self,
        hash: TokenHash,
    ) -> Result<Option<StaffSessionRecord>, RepoError> {
        let mut conn = self.conn().await;
        let bytes = hash.as_bytes().as_slice();
        let sql = format!(
            "SELECT {}, {}, {} FROM sessions s
             LEFT JOIN users u ON s.principal_kind = 'staff' AND u.id = s.principal_id
             LEFT JOIN roles r ON r.id = u.role_id
             WHERE s.token_hash = $1",
            session_columns_as("s", "s_"),
            prefixed_columns("u", "u_", USER_PLAIN, USER_TIMESTAMPS),
            prefixed_columns("r", "r_", ROLE_PLAIN, &[]),
        );
        let Some(row) = conn
            .db()
            .query_typed_opt(&sql, &[(&bytes, Type::BYTEA)])
            .await
            .map_err(repo_error)?
        else {
            return Ok(None);
        };
        let session = session_from(Prefixed {
            row: &row,
            prefix: "s_",
        })?;
        let user = match col::<Option<Uuid>>(&row, "u_id")? {
            Some(_) => Some(user_from(Prefixed {
                row: &row,
                prefix: "u_",
            })?),
            None => None,
        };
        let role = match col::<Option<Uuid>>(&row, "r_id")? {
            Some(_) => Some(role_from(Prefixed {
                row: &row,
                prefix: "r_",
            })?),
            None => None,
        };
        Ok(Some(StaffSessionRecord {
            session,
            user,
            role,
        }))
    }
}

// ---------- roles ----------

const ROLE_PLAIN: &[&str] = &["id", "version", "name", "is_owner", "scopes"];
const ROLE_COLUMNS: &str = "id, version, name, is_owner, scopes";

fn role_from_row(row: &Row) -> Result<Role, RepoError> {
    role_from(Prefixed::plain(row))
}

fn role_from(row: Prefixed<'_>) -> Result<Role, RepoError> {
    let is_owner: bool = row.col("is_owner")?;
    let scopes = if is_owner {
        RoleScopes::All
    } else {
        RoleScopes::Only(scopes_from_db(row.col("scopes")?)?)
    };
    Role::restore(RoleSnapshot {
        id: RoleId::from_uuid(row.col("id")?),
        version: version_from_db(row.col("version")?)?,
        name: row.col("name")?,
        scopes,
    })
    .map_err(corrupt)
}

/// `(is_owner, scopes)`; the owner's scopes column is empty.
fn role_scopes_to_db(scopes: &RoleScopes) -> (bool, Vec<&'static str>) {
    match scopes {
        RoleScopes::All => (true, Vec::new()),
        RoleScopes::Only(scopes) => (false, scopes_to_db(scopes)),
    }
}

async fn select_role(
    client: &impl Queries,
    filter: &str,
    params: &[(&(dyn tokio_postgres::types::ToSql + Sync), Type)],
) -> Result<Option<Role>, RepoError> {
    client
        .query_typed_opt(
            &format!("SELECT {ROLE_COLUMNS} FROM roles WHERE {filter}"),
            params,
        )
        .await
        .map_err(repo_error)?
        .map(|row| role_from_row(&row))
        .transpose()
}

impl<C: PgConnection> RoleRepo for PgStore<C> {
    async fn get(&self, id: RoleId) -> Result<Option<Role>, RepoError> {
        let mut conn = self.conn().await;
        select_role(&conn.db(), "id = $1", &[(&id.as_uuid(), Type::UUID)]).await
    }

    async fn get_by_name(&self, name: &str) -> Result<Option<Role>, RepoError> {
        let mut conn = self.conn().await;
        let name = name.trim();
        select_role(
            &conn.db(),
            "lower(name) = lower($1)",
            &[(&name, Type::TEXT)],
        )
        .await
    }

    async fn owner(&self) -> Result<Option<Role>, RepoError> {
        let mut conn = self.conn().await;
        select_role(&conn.db(), "is_owner", &[]).await
    }

    async fn list_all(&self) -> Result<Vec<Role>, RepoError> {
        let mut conn = self.conn().await;
        conn.db()
            .query_typed(
                &format!("SELECT {ROLE_COLUMNS} FROM roles ORDER BY id"),
                &[],
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(role_from_row)
            .collect()
    }

    async fn insert(&self, role: &mut Role) -> Result<(), RepoError> {
        let s = role.snapshot();
        in_transaction!(self, |tx| insert_role(tx, &s).await)?;
        role.set_version(Version::NEW.next());
        Ok(())
    }

    async fn update(&self, role: &mut Role) -> Result<(), RepoError> {
        let s = role.snapshot();
        let version = in_transaction!(self, |tx| update_role(tx, &s).await)?;
        role.set_version(version);
        Ok(())
    }
}

pub(crate) async fn update_role(
    client: &impl Queries,
    s: &RoleSnapshot,
) -> Result<Version, RepoError> {
    let (is_owner, scopes) = role_scopes_to_db(&s.scopes);
    let row = client
        .query_typed_opt(
            "UPDATE roles SET version = version + 1, name = $3, is_owner = $4, scopes = $5
             WHERE id = $1 AND version = $2 RETURNING version",
            &[
                (&s.id.as_uuid(), Type::UUID),
                (&version_to_db(s.version)?, Type::INT8),
                (&s.name, Type::TEXT),
                (&is_owner, Type::BOOL),
                (&scopes, Type::TEXT_ARRAY),
            ],
        )
        .await
        .map_err(repo_error)?;
    let Some(row) = row else {
        return Err(RepoError::StaleVersion);
    };
    version_from_db(col(&row, "version")?)
}

pub(crate) async fn insert_role(client: &impl Queries, s: &RoleSnapshot) -> Result<(), RepoError> {
    let (is_owner, scopes) = role_scopes_to_db(&s.scopes);
    client
        .query_typed(
            "INSERT INTO roles (id, version, name, is_owner, scopes) VALUES ($1, 1, $2, $3, $4)",
            &[
                (&s.id.as_uuid(), Type::UUID),
                (&s.name, Type::TEXT),
                (&is_owner, Type::BOOL),
                (&scopes, Type::TEXT_ARRAY),
            ],
        )
        .await
        .map_err(repo_error)?;
    Ok(())
}

// ---------- invites ----------

fn invite_columns() -> String {
    format!(
        "id, version, email, role_id, token_hash, invited_by_kind, invited_by_id, {}, {}, {}",
        micros("created_at"),
        micros("expires_at"),
        micros("accepted_at")
    )
}

fn invite_from_row(row: &Row) -> Result<Invite, RepoError> {
    let email: String = col(row, "email")?;
    let by_kind: String = col(row, "invited_by_kind")?;
    let accepted: Option<i64> = col(row, "accepted_at")?;
    Ok(Invite::restore(InviteSnapshot {
        id: InviteId::from_uuid(col(row, "id")?),
        version: version_from_db(col(row, "version")?)?,
        email: Email::new(&email).map_err(corrupt)?,
        role: RoleId::from_uuid(col(row, "role_id")?),
        token_hash: token_hash_from_db(col(row, "token_hash")?)?,
        invited_by: actor_from_db(&by_kind, col(row, "invited_by_id")?)?,
        created_at: Timestamp::from_unix_micros(col(row, "created_at")?),
        expires_at: Timestamp::from_unix_micros(col(row, "expires_at")?),
        accepted_at: accepted.map(Timestamp::from_unix_micros),
    }))
}

async fn select_invite(
    client: &impl Queries,
    filter: &str,
    value: &(dyn tokio_postgres::types::ToSql + Sync),
    ty: Type,
) -> Result<Option<Invite>, RepoError> {
    client
        .query_typed_opt(
            &format!("SELECT {} FROM invites WHERE {filter}", invite_columns()),
            &[(value, ty)],
        )
        .await
        .map_err(repo_error)?
        .map(|row| invite_from_row(&row))
        .transpose()
}

impl<C: PgConnection> InviteRepo for PgStore<C> {
    async fn get(&self, id: InviteId) -> Result<Option<Invite>, RepoError> {
        let mut conn = self.conn().await;
        select_invite(&conn.db(), "id = $1", &id.as_uuid(), Type::UUID).await
    }

    async fn get_by_email(&self, email: &Email) -> Result<Option<Invite>, RepoError> {
        let mut conn = self.conn().await;
        select_invite(&conn.db(), "email = $1", &email.as_str(), Type::TEXT).await
    }

    async fn get_by_token(&self, hash: TokenHash) -> Result<Option<Invite>, RepoError> {
        let mut conn = self.conn().await;
        let bytes = hash.as_bytes().as_slice();
        select_invite(&conn.db(), "token_hash = $1", &bytes, Type::BYTEA).await
    }

    async fn pending(&self) -> Result<Vec<Invite>, RepoError> {
        let mut conn = self.conn().await;
        conn.db()
            .query_typed(
                &format!(
                    "SELECT {} FROM invites WHERE accepted_at IS NULL ORDER BY id",
                    invite_columns()
                ),
                &[],
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(invite_from_row)
            .collect()
    }

    async fn insert(&self, invite: &mut Invite) -> Result<(), RepoError> {
        let s = invite.snapshot();
        in_transaction!(self, |tx| insert_invite(tx, &s).await)?;
        invite.set_version(Version::NEW.next());
        Ok(())
    }

    async fn update(&self, invite: &mut Invite) -> Result<(), RepoError> {
        let s = invite.snapshot();
        let version = in_transaction!(self, |tx| update_invite(tx, &s).await)?;
        invite.set_version(version);
        Ok(())
    }

    async fn delete(&self, invite: &Invite) -> Result<(), RepoError> {
        let mut conn = self.conn().await;
        let deleted = conn
            .db()
            .execute_typed(
                "DELETE FROM invites WHERE id = $1 AND version = $2",
                &[
                    (&invite.id().as_uuid(), Type::UUID),
                    (&version_to_db(invite.version())?, Type::INT8),
                ],
            )
            .await
            .map_err(repo_error)?;
        if deleted == 0 {
            return Err(RepoError::StaleVersion);
        }
        Ok(())
    }
}

/// `InviteRepo::insert` inside the caller's transaction.
pub(crate) async fn insert_invite(tx: &impl Queries, s: &InviteSnapshot) -> Result<(), RepoError> {
    let (by_kind, by_id) = actor_to_db(s.invited_by);
    tx.query_typed(
        &format!(
            "INSERT INTO invites (id, version, email, role_id, token_hash,
                 invited_by_kind, invited_by_id, created_at, expires_at, accepted_at)
             VALUES ($1, 1, $2, $3, $4, $5, $6, {}, {}, {})",
            ts(7),
            ts(8),
            ts(9)
        ),
        &[
            (&s.id.as_uuid(), Type::UUID),
            (&s.email.as_str(), Type::TEXT),
            (&s.role.as_uuid(), Type::UUID),
            (&s.token_hash.as_bytes().as_slice(), Type::BYTEA),
            (&by_kind, Type::TEXT),
            (&by_id, Type::UUID),
            (&s.created_at.unix_micros(), Type::INT8),
            (&s.expires_at.unix_micros(), Type::INT8),
            (&s.accepted_at.map(Timestamp::unix_micros), Type::INT8),
        ],
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}

pub(crate) async fn update_invite(
    client: &impl Queries,
    s: &InviteSnapshot,
) -> Result<Version, RepoError> {
    let (by_kind, by_id) = actor_to_db(s.invited_by);
    let row = client
        .query_typed_opt(
            &format!(
                "UPDATE invites SET version = version + 1, role_id = $3, token_hash = $4,
                     invited_by_kind = $5, invited_by_id = $6, created_at = {},
                     expires_at = {}, accepted_at = {}
                 WHERE id = $1 AND version = $2 RETURNING version",
                ts(7),
                ts(8),
                ts(9)
            ),
            &[
                (&s.id.as_uuid(), Type::UUID),
                (&version_to_db(s.version)?, Type::INT8),
                (&s.role.as_uuid(), Type::UUID),
                (&s.token_hash.as_bytes().as_slice(), Type::BYTEA),
                (&by_kind, Type::TEXT),
                (&by_id, Type::UUID),
                (&s.created_at.unix_micros(), Type::INT8),
                (&s.expires_at.unix_micros(), Type::INT8),
                (&s.accepted_at.map(Timestamp::unix_micros), Type::INT8),
            ],
        )
        .await
        .map_err(repo_error)?;
    let Some(row) = row else {
        return Err(RepoError::StaleVersion);
    };
    version_from_db(col(&row, "version")?)
}
