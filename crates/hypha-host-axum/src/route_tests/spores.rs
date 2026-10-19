//! A host built with a spore (design note 0030, §9): `deployment!` with a test-only spore,
//! started the way the binary starts (`start::prepare`). The stock build has none.

use hypha_apps::{AppKind, AppRepo, HasApps};
use hypha_core::{AppKey, EventHandler, EventHandlers, StoredEvent};
use hypha_spore_core::Spore;
use hypha_store_postgres::{Migration, MigrationSet};

use super::*;
use hypha_http::state::Storage;

use crate::{Deployment, PgPorts};

/// Handles nothing, on any ports.
struct Noop;

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
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct TestConfig {
    #[serde(default)]
    greeting: Option<String>,
}

/// One table, one handler, no payment providers; it reads its configuration section.
struct TestSpore {
    greeting: String,
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

    fn new(env: hypha_spore_core::SporeEnv) -> Result<Self, hypha_spore_core::SporeError> {
        let config: TestConfig = env.config_as()?;
        Ok(TestSpore {
            greeting: config.greeting.unwrap_or_else(|| "hello".to_string()),
        })
    }

    type PaymentProviders = ();
    type Handlers = (Noop,);

    fn payment_providers(&self) {}

    fn handlers(&self) -> (Noop,) {
        (Noop,)
    }
}

crate::deployment! { enum WithTestSpore { spores: [TestSpore] } }

/// The test spore built from `toml` (its `hypha.config.toml`), on a fresh schema.
async fn storage_from(toml: &str) -> Result<crate::PgStorage<WithTestSpore>, String> {
    let config = Config::load(Some(toml), |_| None).map_err(|e| e.to_string())?;
    let runtime = crate::platform::runtime();
    let db = PgDatabase::connect(
        &fresh_schema().await,
        &config.database,
        runtime.clock.clone(),
        &crate::migrations::<WithTestSpore>(),
    )
    .await?;
    let env = HostEnv::new([7; 32], config.auth, runtime.clone());
    let outbound = hypha_http::outbound::OutboundPolicy::default();
    let client = crate::calls::client(outbound)?;
    let spores = hypha_http::spores::spore_host(&env, crate::calls::host_calls(client), outbound);
    let mail = Arc::new(MailHost::new(
        MailParts {
            sender: HostMail::log(),
            from: config.email.from.clone(),
            store: hypha_core::StoreId::from_uuid(uuid::Uuid::nil()),
            admin_url: None,
            order_tokens: env.order_tokens().clone(),
            media: None,
            queued: MailQueued::none(),
        },
        &runtime,
    ));
    crate::PgStorage::<WithTestSpore>::new(db, mail, &spores, &config.spores)
        .map_err(|e| e.to_string())
}

#[tokio::test]
async fn a_spore_reads_its_section_and_one_it_refuses_stops_the_host() {
    let storage = storage_from("[spores.test]\ngreeting = \"hi\"")
        .await
        .unwrap();
    let (spore,) = storage.spores();
    assert_eq!(spore.greeting, "hi");
    let refused = storage_from("[spores.test]\ngreting = \"hi\"").await.err();
    let refused = refused.unwrap_or_default();
    assert!(
        refused.starts_with("spore \"test\": unknown field `greting`"),
        "{refused}"
    );
}

#[tokio::test]
async fn a_host_built_with_a_spore_migrates_registers_and_runs_it() {
    let config = Config::defaults();
    let runtime = crate::platform::runtime();
    let url = fresh_schema().await;
    let db = PgDatabase::connect(
        &url,
        &config.database,
        runtime.clock.clone(),
        &crate::migrations::<WithTestSpore>(),
    )
    .await
    .unwrap();
    let env =
        HostEnv::new([7; 32], config.auth, runtime.clone()).with_hasher(Argon2id::new(8, 1, 1));
    // As the binary starts: the store, the spore's app, the owner, the location, search.
    let prepared =
        crate::start::prepare::<WithTestSpore>(db, env, HostMail::log(), runtime, config).await;
    let state = prepared.state;

    // Its handler runs beside core's.
    let names = EventHandlers::<PgPorts>::names(state.storage.handlers());
    assert!(names.contains(&"spore_test_noop"), "{names:?}");
    assert!(names.contains(&"variant_cleanup"), "{names:?}");
    assert_eq!(
        EventHandlers::<PgPorts>::wanting(state.storage.handlers(), "product.created"),
        ["search_index", "spore_test_noop"]
    );

    // Its app, enabled, its scopes its grant.
    let ports = state.public_ports().await.unwrap();
    let key = AppKey::of_spore("test").unwrap();
    let app = ports.apps().get(&key).await.unwrap().unwrap();
    assert_eq!(app.kind(), AppKind::Spore);
    assert_eq!(app.name(), "Test spore");
    assert!(app.enabled());
    assert_eq!(app.grant().unwrap().scopes(), &[Scope::ProductsRead]);

    // Its table, migrated after core's.
    let config = hypha_store_postgres::database_config(&url).unwrap();
    let (client, connection) = config.connect(tokio_postgres::NoTls).await.unwrap();
    tokio::spawn(connection);
    let row = client
        .query_one("SELECT to_regclass('spore_test_notes') IS NOT NULL", &[])
        .await
        .unwrap();
    assert!(row.get::<_, bool>(0));

    // The deployment's router lists it among the apps.
    let ports = state.public_ports().await.unwrap();
    let input = CreateApiKey {
        kind: ApiKeyKind::Secret,
        name: "Tests".to_string(),
        scopes: Some(Scope::ALL.to_vec()),
        channel: None,
    };
    let (_, token) = create_api_key(&Ctx::system(), &ports, &state.env, input)
        .await
        .unwrap();
    let router = WithTestSpore::router()
        .layer(axum::middleware::from_fn(crate::start::record_peer))
        .with_state(Arc::clone(&state))
        .layer(MockConnectInfo(SocketAddr::from(PEER)));
    let request = Request::builder()
        .uri("/admin/apps")
        .header(AUTHORIZATION, format!("Bearer {}", token.expose()))
        .body(Body::empty())
        .unwrap();
    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let list: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(list["items"][0]["key"], "spore_test", "{list}");
    assert_eq!(list["items"][0]["kind"], "spore", "{list}");
}
