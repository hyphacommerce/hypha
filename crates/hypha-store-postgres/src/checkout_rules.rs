//! Checkout rules: where each applies, its conditions and its outcome kept as JSON in the
//! API's shape, read back through the rule's constructor rules.

use hypha_checkout_rules::{
    CheckoutRule, CheckoutRuleRepo, CheckoutRuleSnapshot, CheckoutSettings, CheckoutSettingsRepo,
    CheckoutSettingsSnapshot, RuleAppliesTo, RuleCondition, RuleDraft, RuleOutcome,
};
use hypha_core::{
    AppKey, CheckoutRuleId, PendingEvent, Recorded, RepoError, Version, Versioned, stamp_insert,
    stamp_update,
};
use serde::{Serialize, de::DeserializeOwned};
use tokio_postgres::{Row, types::Type};

use crate::{
    columns::{col, stamp_columns, stamps_from_row, version_from_db, version_to_db, write_stamps},
    error::{corrupt, repo_error},
    outbox::insert_events,
    store::{PgConnection, PgStore, Queries, in_transaction},
};

fn columns() -> String {
    format!(
        "id, version, name, enabled, position, applies_to::text AS applies_to, \
         conditions::text AS conditions, outcome::text AS outcome, {}",
        stamp_columns("")
    )
}

fn json_from<T: DeserializeOwned>(
    row: &Row,
    column: &str,
    id: CheckoutRuleId,
) -> Result<T, RepoError> {
    let text: String = col(row, column)?;
    serde_json::from_str(&text).map_err(|e| corrupt(format!("checkout rule {id} {column}: {e}")))
}

fn json_to<T: Serialize>(value: &T) -> Result<String, RepoError> {
    serde_json::to_string(value).map_err(|e| corrupt(format!("checkout rule: {e}")))
}

fn rule_from_row(row: &Row) -> Result<CheckoutRule, RepoError> {
    let id = CheckoutRuleId::from_uuid(col(row, "id")?);
    let position: i32 = col(row, "position")?;
    let applies_to: RuleAppliesTo = json_from(row, "applies_to", id)?;
    let conditions: Vec<RuleCondition> = json_from(row, "conditions", id)?;
    let outcome: RuleOutcome = json_from(row, "outcome", id)?;
    CheckoutRule::restore(CheckoutRuleSnapshot {
        id,
        version: version_from_db(col(row, "version")?)?,
        stamps: stamps_from_row(row)?,
        position: u32::try_from(position)
            .map_err(|_| corrupt(format!("checkout rule {id} position {position}")))?,
        draft: RuleDraft {
            name: col(row, "name")?,
            enabled: col(row, "enabled")?,
            applies_to,
            conditions,
            outcome,
        },
    })
    .map_err(|e| corrupt(format!("checkout rule {id}: {e}")))
}

/// A rule's columns as owned values, so the parameters can borrow them.
struct Values {
    id: uuid::Uuid,
    name: String,
    enabled: bool,
    position: i32,
    applies_to: String,
    conditions: String,
    outcome: String,
}

impl Values {
    fn of(s: &CheckoutRuleSnapshot) -> Result<Self, RepoError> {
        Ok(Self {
            id: s.id.as_uuid(),
            name: s.draft.name.clone(),
            enabled: s.draft.enabled,
            position: i32::try_from(s.position)
                .map_err(|_| corrupt(format!("checkout rule position {}", s.position)))?,
            applies_to: json_to(&s.draft.applies_to)?,
            conditions: json_to(&s.draft.conditions)?,
            outcome: json_to(&s.draft.outcome)?,
        })
    }
}

/// Updates one rule in the caller's transaction when it's still at `version`.
async fn update_rule(
    tx: &impl Queries,
    v: &Values,
    version: i64,
    s: &CheckoutRuleSnapshot,
) -> Result<Version, RepoError> {
    let row = tx
        .query_typed_opt(
            "UPDATE checkout_rules SET version = version + 1, name = $3, enabled = $4,
                 position = $5, applies_to = $6::text::jsonb, conditions = $7::text::jsonb,
                 outcome = $8::text::jsonb
             WHERE id = $1 AND version = $2 RETURNING version",
            &[
                (&v.id, Type::UUID),
                (&version, Type::INT8),
                (&v.name, Type::TEXT),
                (&v.enabled, Type::BOOL),
                (&v.position, Type::INT4),
                (&v.applies_to, Type::TEXT),
                (&v.conditions, Type::TEXT),
                (&v.outcome, Type::TEXT),
            ],
        )
        .await
        .map_err(repo_error)?
        .ok_or(RepoError::StaleVersion)?;
    write_stamps(tx, "checkout_rules", "id", &v.id, Type::UUID, s.stamps).await?;
    version_from_db(col(&row, "version")?)
}

impl<C: PgConnection> CheckoutRuleRepo for PgStore<C> {
    async fn get(&self, id: CheckoutRuleId) -> Result<Option<CheckoutRule>, RepoError> {
        let mut conn = self.conn().await;
        let row = conn
            .db()
            .query_typed_opt(
                &format!("SELECT {} FROM checkout_rules WHERE id = $1", columns()),
                &[(&id.as_uuid(), Type::UUID)],
            )
            .await
            .map_err(repo_error)?;
        row.as_ref().map(rule_from_row).transpose()
    }

    async fn list(&self) -> Result<Vec<CheckoutRule>, RepoError> {
        let mut conn = self.conn().await;
        conn.db()
            .query_typed(
                &format!(
                    "SELECT {} FROM checkout_rules ORDER BY position, id",
                    columns()
                ),
                &[],
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(rule_from_row)
            .collect()
    }

    async fn insert(&self, rule: &mut CheckoutRule) -> Result<(), RepoError> {
        stamp_insert(rule, self.now());
        let s = rule.snapshot();
        let events = rule.take_events();
        let stamp = self.event_stamp();
        let version = Version::NEW.next();
        let v = Values::of(&s)?;
        in_transaction!(self, |tx| {
            tx.query_typed(
                "INSERT INTO checkout_rules (id, version, name, enabled, position, applies_to,
                     conditions, outcome)
                 VALUES ($1, 1, $2, $3, $4, $5::text::jsonb, $6::text::jsonb, $7::text::jsonb)",
                &[
                    (&v.id, Type::UUID),
                    (&v.name, Type::TEXT),
                    (&v.enabled, Type::BOOL),
                    (&v.position, Type::INT4),
                    (&v.applies_to, Type::TEXT),
                    (&v.conditions, Type::TEXT),
                    (&v.outcome, Type::TEXT),
                ],
            )
            .await
            .map_err(repo_error)?;
            write_stamps(tx, "checkout_rules", "id", &v.id, Type::UUID, s.stamps).await?;
            insert_events(tx, &events, Some(version), stamp).await
        })?;
        rule.set_version(version);
        Ok(())
    }

    async fn update_many(&self, rules: &mut [CheckoutRule]) -> Result<(), RepoError> {
        if rules.is_empty() {
            return Ok(());
        }
        let now = self.now();
        let mut written: Vec<(CheckoutRuleSnapshot, Values, i64, Vec<PendingEvent>)> =
            Vec::with_capacity(rules.len());
        for rule in rules.iter_mut() {
            stamp_update(rule, now);
            let s = rule.snapshot();
            let values = Values::of(&s)?;
            let version = version_to_db(s.version)?;
            written.push((s, values, version, rule.take_events()));
        }
        let stamp = self.event_stamp();
        let versions = in_transaction!(self, |tx| {
            let mut versions = Vec::with_capacity(written.len());
            for (s, values, version, events) in &written {
                let new = update_rule(tx, values, *version, s).await?;
                insert_events(tx, events, Some(new), stamp).await?;
                versions.push(new);
            }
            Ok(versions)
        })?;
        for (rule, version) in rules.iter_mut().zip(versions) {
            rule.set_version(version);
        }
        Ok(())
    }

    async fn delete(&self, rule: &mut CheckoutRule) -> Result<(), RepoError> {
        let id = rule.id().as_uuid();
        let version = version_to_db(rule.version())?;
        let events = rule.take_events();
        let stamp = self.event_stamp();
        in_transaction!(self, |tx| {
            let deleted = tx
                .query_typed_opt(
                    "DELETE FROM checkout_rules WHERE id = $1 AND version = $2 RETURNING id",
                    &[(&id, Type::UUID), (&version, Type::INT8)],
                )
                .await
                .map_err(repo_error)?;
            if deleted.is_none() {
                return Err(RepoError::StaleVersion);
            }
            insert_events(tx, &events, None, stamp).await
        })
    }
}

impl<C: PgConnection> CheckoutSettingsRepo for PgStore<C> {
    async fn get(&self) -> Result<Option<CheckoutSettings>, RepoError> {
        let mut conn = self.conn().await;
        let row = conn
            .db()
            .query_typed_opt(
                &format!(
                    "SELECT version, verification_app, autocomplete_app,
                            validation_apps::text AS validation_apps, {}
                     FROM checkout_settings",
                    stamp_columns("")
                ),
                &[],
            )
            .await
            .map_err(repo_error)?;
        let Some(row) = row else { return Ok(None) };
        let app = |column: &str| -> Result<Option<AppKey>, RepoError> {
            let key: Option<String> = col(&row, column)?;
            key.map(|a| AppKey::new(&a))
                .transpose()
                .map_err(|e| corrupt(format!("checkout settings' {column}: {e}")))
        };
        Ok(Some(CheckoutSettings::restore(CheckoutSettingsSnapshot {
            version: version_from_db(col(&row, "version")?)?,
            stamps: stamps_from_row(&row)?,
            verification_app: app("verification_app")?,
            autocomplete_app: app("autocomplete_app")?,
            validation_apps: serde_json::from_str(&col::<String>(&row, "validation_apps")?)
                .map_err(|e| corrupt(format!("checkout settings' validation_apps: {e}")))?,
        })))
    }

    async fn insert(&self, settings: &mut CheckoutSettings) -> Result<(), RepoError> {
        stamp_insert(settings, self.now());
        let s = settings.snapshot();
        let events = settings.take_events();
        let stamp = self.event_stamp();
        let version = Version::NEW.next();
        let app = s.verification_app.as_ref().map(|a| a.as_str().to_string());
        let autocomplete = s.autocomplete_app.as_ref().map(|a| a.as_str().to_string());
        let validators = validation_apps_json(&s)?;
        in_transaction!(self, |tx| {
            tx.query_typed(
                "INSERT INTO checkout_settings
                     (version, verification_app, autocomplete_app, validation_apps)
                 VALUES (1, $1, $2, $3::text::jsonb)",
                &[
                    (&app, Type::TEXT),
                    (&autocomplete, Type::TEXT),
                    (&validators, Type::TEXT),
                ],
            )
            .await
            .map_err(repo_error)?;
            write_stamps(tx, "checkout_settings", "id", &true, Type::BOOL, s.stamps).await?;
            insert_events(tx, &events, Some(version), stamp).await
        })?;
        settings.set_version(version);
        Ok(())
    }

    async fn update(&self, settings: &mut CheckoutSettings) -> Result<(), RepoError> {
        stamp_update(settings, self.now());
        let s = settings.snapshot();
        let events = settings.take_events();
        let stamp = self.event_stamp();
        let app = s.verification_app.as_ref().map(|a| a.as_str().to_string());
        let autocomplete = s.autocomplete_app.as_ref().map(|a| a.as_str().to_string());
        let validators = validation_apps_json(&s)?;
        let current = version_to_db(s.version)?;
        let version = in_transaction!(self, |tx| {
            let row = tx
                .query_typed_opt(
                    "UPDATE checkout_settings SET version = version + 1, verification_app = $2,
                         autocomplete_app = $3, validation_apps = $4::text::jsonb
                     WHERE version = $1 RETURNING version",
                    &[
                        (&current, Type::INT8),
                        (&app, Type::TEXT),
                        (&autocomplete, Type::TEXT),
                        (&validators, Type::TEXT),
                    ],
                )
                .await
                .map_err(repo_error)?
                .ok_or(RepoError::StaleVersion)?;
            write_stamps(tx, "checkout_settings", "id", &true, Type::BOOL, s.stamps).await?;
            let version = version_from_db(col(&row, "version")?)?;
            insert_events(tx, &events, Some(version), stamp).await?;
            Ok(version)
        })?;
        settings.set_version(version);
        Ok(())
    }
}

fn validation_apps_json(s: &CheckoutSettingsSnapshot) -> Result<String, RepoError> {
    serde_json::to_string(&s.validation_apps)
        .map_err(|e| corrupt(format!("checkout settings' validation_apps: {e}")))
}
