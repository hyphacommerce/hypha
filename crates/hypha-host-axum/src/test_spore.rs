//! A spore for the route tests (design note 0030): one table, one event handler, a
//! configuration section, settings with a secret, and a route on each surface
//! (`crate::test_spore_routes`, declared at the crate root as `spore_routes!` asks).
//! The stock build has no spores.

use axum::{Json, extract::State};
use hypha_core::{EventHandler, Scope, StoredEvent};
use hypha_http::__private::utoipa;
use hypha_http::error::ApiError;
use hypha_http::extract::{Caller, Storefront};
use hypha_http::state::{SharedState, Storage};
use hypha_spore_core::{Spore, SporeEnv, SporeError};
use hypha_store_postgres::{Migration, MigrationSet};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

/// Handles nothing, on any ports.
pub struct Noop;

impl<P> EventHandler<P> for Noop {
    fn name(&self) -> &'static str {
        "spore_test_noop"
    }

    fn wants(&self, event_type: &str) -> bool {
        event_type == "product.created"
    }

    async fn handle(&self, _: &P, _: &StoredEvent) -> Result<(), String> {
        Ok(())
    }
}

/// Its section of `hypha.config.toml`, `[spores.test]`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TestConfig {
    #[serde(default)]
    greeting: Option<String>,
}

/// Its settings, as the merchant saves them.
#[derive(Deserialize)]
pub struct TestSettings {
    pub mode: String,
    #[serde(default)]
    pub api_key: Option<String>,
}

pub struct TestSpore {
    pub greeting: String,
    pub env: SporeEnv,
}

impl Spore for TestSpore {
    const NAME: &'static str = "test";
    const SCOPES: &'static [Scope] = &[Scope::ProductsRead];

    fn title() -> &'static str {
        "Test spore"
    }

    fn migrations() -> MigrationSet {
        MigrationSet {
            module: "spore_test",
            migrations: &[Migration {
                version: 1,
                name: "notes",
                sql: "CREATE TABLE spore_test_notes (id uuid PRIMARY KEY, body text NOT NULL);",
            }],
        }
    }

    fn new(env: SporeEnv) -> Result<Self, SporeError> {
        let config: TestConfig = env.config_as()?;
        Ok(TestSpore {
            greeting: config.greeting.unwrap_or_else(|| "hello".to_string()),
            env,
        })
    }

    fn settings_schema() -> Option<Value> {
        Some(json!({
            "type": "object",
            "title": "Test spore",
            "properties": {
                "api_key": { "type": "string", "title": "API key", "x-hypha-secret": true },
                "mode": { "type": "string", "title": "Mode", "enum": ["test", "live"] }
            },
            "required": ["mode"]
        }))
    }

    fn check_settings(
        &self,
        settings: &Map<String, Value>,
    ) -> Result<(), Vec<hypha_apps::SettingsProblem>> {
        match settings.get("api_key").and_then(Value::as_str) {
            Some(key) if !key.starts_with("key_") => Err(vec![hypha_apps::SettingsProblem::new(
                "api_key",
                "starts with key_",
            )]),
            _ => Ok(()),
        }
    }

    type PaymentProviders = ();
    type Handlers = (Noop,);

    fn payment_providers(&self) {}

    fn handlers(&self) -> (Noop,) {
        (Noop,)
    }
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct Greeting {
    greeting: String,
}

/// Its greeting, to anyone.
#[utoipa::path(
    get,
    path = "/spores/test/hello",
    responses((status = 200, description = "OK", body = Greeting)),
    security(()),
)]
pub async fn hello<S: Storage>(
    State(state): State<SharedState<S>>,
) -> Result<Json<Greeting>, ApiError> {
    let spore = hypha_http::spores::spore::<TestSpore, S>(&state)?;
    Ok(Json(Greeting {
        greeting: spore.greeting.clone(),
    }))
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct SettingsSeen {
    mode: Option<String>,
    /// The secret as the spore reads it: opened.
    api_key: Option<String>,
}

/// What the spore reads of its settings (staff with `apps:read`).
#[utoipa::path(
    get,
    path = "/admin/spores/test/settings",
    responses((status = 200, description = "OK", body = SettingsSeen)),
    extensions(("x-hypha-scopes" = json!(["apps:read"]))),
)]
pub async fn settings<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
) -> Result<Json<SettingsSeen>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    ctx.require(Scope::AppsRead)
        .map_err(hypha_apps::CommandError::from)?;
    let spore = hypha_http::spores::spore::<TestSpore, S>(&state)?;
    let settings: Option<TestSettings> = spore
        .env
        .settings(&ports)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(SettingsSeen {
        mode: settings.as_ref().map(|s| s.mode.clone()),
        api_key: settings.and_then(|s| s.api_key),
    }))
}

/// Its greeting, to a storefront.
#[utoipa::path(
    get,
    path = "/storefront/spores/test/greeting",
    responses((status = 200, description = "OK", body = Greeting)),
    extensions(("x-hypha-scopes" = json!([]))),
)]
pub async fn storefront_greeting<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller<Storefront>,
) -> Result<Json<Greeting>, ApiError> {
    let _ = caller.authenticate(&state).await?;
    let spore = hypha_http::spores::spore::<TestSpore, S>(&state)?;
    Ok(Json(Greeting {
        greeting: format!("{}, shopper", spore.greeting),
    }))
}
