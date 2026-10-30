//! A spore's settings: checked against its schema and by the spore, secret fields sealed
//! and never answered, read back unsealed by the spore.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "a failing test panics, and fixtures cast small counters"
)]
use hypha_apps::{
    AppKey, CommandError, SettingsProblem, SettingsSchema, SporeRegistration, SporeSettingsRepo,
    get_spore_settings, read_spore_settings, register_spore, save_spore_settings,
};
use hypha_core::{Actor, Ctx, Scope};
use hypha_testkit::{InMemoryPorts, TestEnv};
use pollster::block_on;
use serde_json::{Map, Value, json};

const STRIPE: SporeRegistration = SporeRegistration {
    name: "stripe",
    title: "Stripe",
    scopes: &[Scope::PaymentsWrite],
};

fn key() -> AppKey {
    AppKey::of_spore("stripe").unwrap()
}

fn schema() -> SettingsSchema {
    SettingsSchema::parse(json!({
        "type": "object",
        "properties": {
            "secret_key": { "type": "string", "x-hypha-secret": true },
            "mode": { "type": "string", "enum": ["test", "live"] }
        },
        "required": ["secret_key", "mode"]
    }))
    .unwrap()
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

fn staff(scopes: &[Scope]) -> Ctx {
    Ctx::new(
        Actor::User(hypha_core::UserId::from_uuid(uuid::Uuid::from_u128(1))),
        scopes.to_vec(),
    )
}

/// The spore's own check: a secret key starts with `sk_`.
fn stripe_check(values: &Map<String, Value>) -> Result<(), Vec<SettingsProblem>> {
    match values.get("secret_key").and_then(Value::as_str) {
        Some(k) if !k.starts_with("sk_") => {
            Err(vec![SettingsProblem::new("secret_key", "starts with sk_")])
        }
        _ => Ok(()),
    }
}

fn save(
    ports: &InMemoryPorts,
    env: &TestEnv,
    input: Value,
) -> Result<Map<String, Value>, CommandError> {
    block_on(save_spore_settings(
        &staff(&[Scope::AppsWrite]),
        ports,
        env,
        "spore_stripe",
        Some(&schema()),
        input,
        stripe_check,
    ))
    .map(|view| view.values)
}

#[test]
fn saved_settings_answer_secrets_as_set_and_the_spore_reads_them_plain() {
    let (ports, env) = registered();
    let answer = save(
        &ports,
        &env,
        json!({ "secret_key": "sk_test_1", "mode": "test" }),
    );
    assert_eq!(
        answer,
        Ok(json!({ "mode": "test", "secret_key": { "set": true } })
            .as_object()
            .unwrap()
            .clone())
    );
    // Stored sealed, never in plain text.
    let stored = block_on(ports.spore_settings.get(&key())).unwrap().unwrap();
    assert!(stored.values()["secret_key"].get("sealed").is_some());
    assert!(!stored.values().values().any(|v| v == "sk_test_1"));
    // The spore reads it back opened.
    let read = block_on(read_spore_settings(&ports, &env, &key())).unwrap();
    assert_eq!(
        read,
        Some(
            json!({ "mode": "test", "secret_key": "sk_test_1" })
                .as_object()
                .unwrap()
                .clone()
        )
    );
}

#[test]
fn a_secret_left_out_is_kept_and_null_clears_it() {
    let (ports, env) = registered();
    save(
        &ports,
        &env,
        json!({ "secret_key": "sk_test_1", "mode": "test" }),
    )
    .unwrap();
    // Changing the mode alone keeps the key, and the spore's check still sees it.
    save(&ports, &env, json!({ "mode": "live" })).unwrap();
    let read = block_on(read_spore_settings(&ports, &env, &key()))
        .unwrap()
        .unwrap();
    assert_eq!(read["secret_key"], "sk_test_1");
    assert_eq!(read["mode"], "live");
    // Clearing a required secret is refused.
    assert_eq!(
        save(&ports, &env, json!({ "secret_key": null, "mode": "live" })),
        Err(CommandError::SettingsInvalid(vec![SettingsProblem::new(
            "secret_key",
            "is required"
        )]))
    );
}

#[test]
fn the_schema_and_the_spores_check_are_both_applied() {
    let (ports, env) = registered();
    assert_eq!(
        save(
            &ports,
            &env,
            json!({ "secret_key": "sk_x", "mode": "staging", "x": 1 })
        ),
        Err(CommandError::SettingsInvalid(vec![
            SettingsProblem::new("mode", "is one of \"test\", \"live\""),
            SettingsProblem::new("x", "is not a setting"),
        ]))
    );
    assert_eq!(
        save(
            &ports,
            &env,
            json!({ "secret_key": "pk_x", "mode": "test" })
        ),
        Err(CommandError::SettingsInvalid(vec![SettingsProblem::new(
            "secret_key",
            "starts with sk_"
        )]))
    );
    assert_eq!(
        save(&ports, &env, json!(["not", "an", "object"])),
        Err(CommandError::SettingsInvalid(vec![SettingsProblem::new(
            "",
            "the settings are a JSON object"
        )]))
    );
    assert_eq!(block_on(ports.spore_settings.get(&key())), Ok(None));
}

#[test]
fn before_a_save_secrets_are_unset_and_only_spores_with_a_schema_have_settings() {
    let (ports, _) = registered();
    let view = block_on(get_spore_settings(
        &staff(&[Scope::AppsRead]),
        &ports,
        "spore_stripe",
        Some(&schema()),
    ))
    .unwrap();
    assert_eq!(
        view.values,
        json!({ "secret_key": { "set": false } })
            .as_object()
            .unwrap()
            .clone()
    );
    assert_eq!(view.version, None);
    assert_eq!(
        block_on(get_spore_settings(
            &staff(&[Scope::AppsRead]),
            &ports,
            "spore_stripe",
            None
        )),
        Err(CommandError::SettingsNotFound(key()))
    );
    for missing in ["spore_reviews", "app_stripe", "nonsense"] {
        assert_eq!(
            block_on(get_spore_settings(
                &staff(&[Scope::AppsRead]),
                &ports,
                missing,
                Some(&schema())
            )),
            Err(CommandError::AppNotFound(missing.to_string())),
            "{missing}"
        );
    }
}

#[test]
fn reading_needs_apps_read_and_saving_apps_write() {
    let (ports, env) = registered();
    assert!(matches!(
        block_on(get_spore_settings(
            &staff(&[]),
            &ports,
            "spore_stripe",
            Some(&schema())
        )),
        Err(CommandError::Auth(_))
    ));
    assert!(matches!(
        block_on(save_spore_settings(
            &staff(&[Scope::AppsRead]),
            &ports,
            &env,
            "spore_stripe",
            Some(&schema()),
            json!({}),
            stripe_check,
        )),
        Err(CommandError::Auth(_))
    ));
}
