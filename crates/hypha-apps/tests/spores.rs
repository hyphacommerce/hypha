//! Spores as apps: registered by the host at startup, changed by staff only by enabling or
//! disabling them.

use hypha_apps::{
    AppError, AppKey, AppKind, CommandError, SporeRegistration, UpdateApp, WebhookInput,
    delete_app, register_spore, rotate_app_secret, spore_registered, update_app,
};
use hypha_core::{Actor, Ctx, Scope, Timestamp};
use hypha_testkit::{InMemoryPorts, TestEnv};
use pollster::block_on;

const STRIPE: SporeRegistration = SporeRegistration {
    name: "stripe",
    title: "Stripe",
    scopes: &[Scope::PaymentsWrite, Scope::OrdersRead],
};

fn key() -> AppKey {
    AppKey::of_spore("stripe").unwrap()
}

fn registered() -> (InMemoryPorts, TestEnv) {
    let ports = InMemoryPorts::default();
    let env = TestEnv::default();
    block_on(register_spore(
        &Ctx::system(),
        &ports.apps,
        &env,
        &env,
        STRIPE,
    ))
    .unwrap();
    (ports, env)
}

fn event_types(ports: &InMemoryPorts) -> Vec<String> {
    ports
        .outbox
        .events()
        .into_iter()
        .map(|e| e.event_type)
        .collect()
}

#[test]
fn a_spore_is_registered_enabled_with_its_scopes_as_its_grant() {
    let (ports, _) = registered();
    let app = block_on(hypha_apps::get_app(
        &Ctx::system(),
        &ports.apps,
        "spore_stripe",
    ))
    .unwrap();
    assert_eq!(app.key(), &key());
    assert_eq!(app.kind(), AppKind::Spore);
    assert_eq!(app.name(), "Stripe");
    assert!(app.enabled());
    assert!(app.is_installed());
    assert!(app.webhook().is_none());
    let grant = app.grant().unwrap();
    assert_eq!(grant.scopes(), &[Scope::OrdersRead, Scope::PaymentsWrite]);
    assert_eq!(grant.approved_by(), Actor::System);
    assert_eq!(event_types(&ports), vec!["app.installed"]);
}

#[test]
fn registering_again_changes_nothing() {
    let (ports, env) = registered();
    let before = block_on(hypha_apps::get_app(
        &Ctx::system(),
        &ports.apps,
        "spore_stripe",
    ))
    .unwrap();
    let again = block_on(register_spore(
        &Ctx::system(),
        &ports.apps,
        &env,
        &env,
        STRIPE,
    ))
    .unwrap();
    assert_eq!(again, before);
    assert_eq!(event_types(&ports), vec!["app.installed"]);
}

#[test]
fn a_new_build_brings_its_title_and_scopes_and_keeps_staffs_switch() {
    let (ports, env) = registered();
    block_on(update_app(
        &Ctx::system(),
        &ports.apps,
        "spore_stripe",
        UpdateApp {
            enabled: Some(false),
            ..UpdateApp::default()
        },
    ))
    .unwrap();
    let wider = SporeRegistration {
        title: "Stripe payments",
        scopes: &[
            Scope::PaymentsWrite,
            Scope::OrdersRead,
            Scope::CustomerDataEmail,
        ],
        ..STRIPE
    };
    let app = block_on(register_spore(
        &Ctx::system(),
        &ports.apps,
        &env,
        &env,
        wider,
    ))
    .unwrap();
    assert_eq!(app.name(), "Stripe payments");
    assert!(app.grant().unwrap().has(Scope::CustomerDataEmail));
    assert!(!app.enabled());
    assert_eq!(
        event_types(&ports),
        vec!["app.installed", "app.grant_changed"]
    );
}

#[test]
fn staff_enable_and_disable_a_spore_and_change_nothing_else() {
    let (ports, env) = registered();
    let ctx = Ctx::system();
    let refused = || Err(CommandError::App(AppError::BuiltIn(key())));
    let disabled = block_on(update_app(
        &ctx,
        &ports.apps,
        "spore_stripe",
        UpdateApp {
            enabled: Some(false),
            ..UpdateApp::default()
        },
    ));
    assert_eq!(disabled.map(|a| a.enabled()), Ok(false));
    for input in [
        UpdateApp {
            name: Some("Mine".to_string()),
            ..UpdateApp::default()
        },
        UpdateApp {
            webhook: Some(Some(WebhookInput {
                url: "https://erp.example/hooks".to_string(),
                timeout_ms: None,
            })),
            ..UpdateApp::default()
        },
        UpdateApp {
            subscriptions: Some(vec!["order.*".to_string()]),
            ..UpdateApp::default()
        },
    ] {
        assert_eq!(
            block_on(update_app(&ctx, &ports.apps, "spore_stripe", input)).map(|_| ()),
            refused()
        );
    }
    assert_eq!(
        block_on(rotate_app_secret(
            &ctx,
            &ports.apps,
            &env,
            &env,
            "spore_stripe"
        ))
        .map(|_| ()),
        refused()
    );
    assert_eq!(
        block_on(delete_app(&ctx, &ports.apps, "spore_stripe")),
        refused()
    );
}

#[test]
fn a_spores_grant_is_the_builds_and_it_cant_be_uninstalled() {
    let (ports, _) = registered();
    let mut app = block_on(hypha_apps::get_app(
        &Ctx::system(),
        &ports.apps,
        "spore_stripe",
    ))
    .unwrap();
    let at = Timestamp::from_unix_micros(1_900_000_000_000_000);
    assert_eq!(
        app.narrow_grant(&[Scope::OrdersRead], Actor::System, at),
        Err(AppError::BuiltIn(key()))
    );
    assert_eq!(app.uninstall(at), Err(AppError::BuiltIn(key())));
}

#[test]
fn registering_needs_apps_write() {
    let ports = InMemoryPorts::default();
    let env = TestEnv::default();
    let ctx = Ctx::new(Actor::System, vec![Scope::AppsRead]);
    assert_eq!(
        block_on(register_spore(&ctx, &ports.apps, &env, &env, STRIPE)).map(|_| ()),
        Err(CommandError::Auth(hypha_core::AuthError::MissingScope(
            Scope::AppsWrite
        )))
    );
}

/// An app repo whose next `get` answers an old copy: what a host that read before
/// another host's write sees.
struct StaleOnce<'a> {
    inner: &'a hypha_testkit::InMemoryAppRepo,
    stale: std::cell::RefCell<Option<hypha_apps::App>>,
}

impl hypha_apps::AppRepo for StaleOnce<'_> {
    async fn get(&self, key: &AppKey) -> Result<Option<hypha_apps::App>, hypha_core::RepoError> {
        // Taken before awaiting: a `RefCell` borrow mustn't be held across an await.
        let stale = self.stale.borrow_mut().take();
        match stale {
            Some(app) => Ok(Some(app)),
            None => self.inner.get(key).await,
        }
    }
    async fn get_by_id(
        &self,
        id: hypha_core::AppId,
    ) -> Result<Option<hypha_apps::App>, hypha_core::RepoError> {
        self.inner.get_by_id(id).await
    }
    async fn list(&self) -> Result<Vec<hypha_apps::App>, hypha_core::RepoError> {
        self.inner.list().await
    }
    async fn insert(&self, app: &mut hypha_apps::App) -> Result<(), hypha_core::RepoError> {
        self.inner.insert(app).await
    }
    async fn update(&self, app: &mut hypha_apps::App) -> Result<(), hypha_core::RepoError> {
        self.inner.update(app).await
    }
    async fn delete(&self, key: &AppKey) -> Result<(), hypha_core::RepoError> {
        self.inner.delete(key).await
    }
}

#[test]
fn a_host_that_lost_the_race_to_update_a_spore_reads_it_again() {
    use hypha_apps::AppRepo;
    let (ports, env) = registered();
    let wider = SporeRegistration {
        scopes: &[
            Scope::PaymentsWrite,
            Scope::OrdersRead,
            Scope::CustomerDataEmail,
        ],
        ..STRIPE
    };
    // Both replicas read version 1; the first writes version 2.
    let stale = block_on(ports.apps.get(&key())).unwrap();
    block_on(register_spore(
        &Ctx::system(),
        &ports.apps,
        &env,
        &env,
        wider,
    ))
    .unwrap();
    let racing = StaleOnce {
        inner: &ports.apps,
        stale: std::cell::RefCell::new(stale),
    };
    let app = block_on(register_spore(&Ctx::system(), &racing, &env, &env, wider)).unwrap();
    assert!(app.grant().unwrap().has(Scope::CustomerDataEmail));
    assert_eq!(
        event_types(&ports),
        vec!["app.installed", "app.grant_changed"]
    );
}

#[test]
fn a_spore_is_registered_only_as_its_build_declares_it() {
    let (ports, _) = registered();
    assert_eq!(block_on(spore_registered(&ports.apps, STRIPE)), Ok(true));
    let wider = SporeRegistration {
        scopes: &[
            Scope::PaymentsWrite,
            Scope::OrdersRead,
            Scope::CustomerDataEmail,
        ],
        ..STRIPE
    };
    assert_eq!(block_on(spore_registered(&ports.apps, wider)), Ok(false));
    let other = SporeRegistration {
        name: "paypal",
        ..STRIPE
    };
    assert_eq!(block_on(spore_registered(&ports.apps, other)), Ok(false));
}

#[test]
fn a_key_belongs_to_the_spore_it_names_or_extends() {
    let (ports, _) = registered();
    let owner = |key: &str| {
        block_on(hypha_apps::spore_of(&ports.apps, key))
            .unwrap()
            .map(|app| app.key().to_string())
    };
    assert_eq!(owner("spore_stripe"), Some("spore_stripe".to_string()));
    assert_eq!(owner("spore_stripe_link"), Some("spore_stripe".to_string()));
    assert_eq!(
        owner("spore_stripe_link_eu"),
        Some("spore_stripe".to_string())
    );
    assert_eq!(owner("spore_stripes"), None);
    assert_eq!(owner("spore_paypal"), None);
    assert_eq!(owner("app_stripe"), None);
    assert_eq!(
        block_on(hypha_apps::spore_enabled(&ports.apps, "spore_stripe_link")),
        Ok(true)
    );
    assert_eq!(
        block_on(hypha_apps::spore_enabled(&ports.apps, "spore_paypal")),
        Ok(false)
    );
}
