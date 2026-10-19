//! Spores (design note 0030): trusted Rust crates compiled into the API beside the
//! external apps of design note 0029. This crate is the `Spore` trait a spore implements
//! and `SporeSet`, the list of spores a host is built with (`host! { spores: [...] }`).
//!
//! It's what the hosts need: what a spore is called, the scopes it acts with, its
//! tables, how it's built (`SporeEnv`, `env.rs`), and what it adds to the host's payment
//! providers and event handlers. The `hypha-spore` facade re-exports it with everything
//! else a spore may use; spores depend on that.

use std::any::Any;
use std::collections::BTreeMap;

use hypha_apps::{SettingsProblem, SettingsSchema, SporeRegistration};
use hypha_core::{AppKey, Scope, Union};
use hypha_payments::{PaymentProviders, ProviderKey};
use hypha_store_postgres::MigrationSet;

pub mod env;

pub use env::{
    BoxFuture, HttpClient, HttpError, HttpMethod, HttpRequest, HttpResponse, SettingsError,
    SporeEnv, SporeError, SporeHost, SporeHttp, SporeStartError,
};

/// One spore. Built once at startup and kept for the host's life, shared by every
/// request (so `Send + Sync`); everything it contributes goes into the host's tuples
/// (static dispatch, so `Send` stays provable on the native host).
pub trait Spore: Sized + Send + Sync + 'static {
    /// `stripe`: its app key (`spore_stripe`, design note 0030, §3), its migration module
    /// and table prefix (`spore_stripe`, `spore_stripe_*`), its configuration section
    /// (`[spores.stripe]`). A contract: never rename one that has shipped.
    const NAME: &'static str;
    /// What it may do, as `Actor::App` (SP2): its grant, shown to staff.
    const SCOPES: &'static [Scope];

    /// Its title in the admin ("Stripe").
    fn title() -> &'static str;

    /// Its own tables (SP4): module `spore_<NAME>` (the host checks), applied after
    /// core's; `MigrationSet { module: "spore_<NAME>", migrations: &[] }` for none.
    fn migrations() -> MigrationSet;

    /// Builds it, at startup, with the host's environment and its own section of
    /// `hypha.config.toml` (`[spores.<NAME>]`, empty without one). An error stops the
    /// host from starting, naming the spore.
    fn new(env: SporeEnv) -> Result<Self, SporeError>;

    /// Its settings form (design note 0030, §8): a JSON Schema of scalar fields
    /// (`hypha_apps::SettingsSchema`'s subset, checked when the host starts), secret
    /// ones marked `"x-hypha-secret": true`. `None` (the default): it takes no settings.
    fn settings_schema() -> Option<serde_json::Value> {
        None
    }

    /// Its own check of settings that fit the schema, secrets in plain text (a Stripe
    /// key starts with `sk_`): every problem, by field. Runs before they're saved.
    fn check_settings(
        &self,
        settings: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<(), Vec<SettingsProblem>> {
        let _ = settings;
        Ok(())
    }

    /// The payment providers it adds (design note 0027): a tuple of `PaymentProvider`s,
    /// `()` for none. Keys are `spore_<NAME>` (or `spore_<NAME>_…`) and unique in the
    /// host.
    type PaymentProviders: PaymentProviders + Send + Sync + 'static;
    /// The event handlers it adds (design note 0019, §5.1): a tuple of
    /// `EventHandler<P>`s, generic over the ports `P` they need, `()` for none. The host
    /// checks they're handlers for its ports when it's built. Names are unique in the
    /// host; prefix them with the spore's name.
    type Handlers: Send + Sync + 'static;

    fn payment_providers(&self) -> Self::PaymentProviders;
    fn handlers(&self) -> Self::Handlers;
}

/// What the HTTP layer asks of a host's built spores, in-process, by key (design note
/// 0030, §7–§8): their settings forms and checks. `P` is the host's ports (what their
/// UI reads with). Implemented for `()` and the tuples `SporeSet` is.
pub trait SporeRuntime<P>: Send + Sync + 'static {
    /// The built spore called `name`, as `Any` (`spore::<T>` downcasts it): how a
    /// spore's routes reach their spore.
    fn find(&self, name: &str) -> Option<&dyn Any>;
    /// The spore `key`'s settings schema; `None` for no such spore, or one without
    /// settings. (Checked at startup, so a spore's parses.)
    fn settings_schema(&self, key: &AppKey) -> Option<SettingsSchema>;
    /// The spore `key`'s own check of settings (`Spore::check_settings`).
    fn check_settings(
        &self,
        key: &AppKey,
        settings: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<(), Vec<SettingsProblem>>;
}

/// The built spore `T` in `spores`, if the host has it: what a spore's route handlers
/// call (through `hypha_http::spores::spore`) to reach their spore.
pub fn spore<T: Spore, P>(spores: &impl SporeRuntime<P>) -> Option<&T> {
    spores.find(T::NAME)?.downcast_ref()
}

impl<P> SporeRuntime<P> for () {
    fn find(&self, _: &str) -> Option<&dyn Any> {
        None
    }

    fn settings_schema(&self, _: &AppKey) -> Option<SettingsSchema> {
        None
    }

    fn check_settings(
        &self,
        _: &AppKey,
        _: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<(), Vec<SettingsProblem>> {
        Ok(())
    }
}

/// Whether `key` is the spore `S`'s.
fn is<S: Spore>(key: &AppKey) -> bool {
    key.as_str().strip_prefix("spore_") == Some(S::NAME)
}

/// `SporeRuntime` for tuples of 1 to 8 spores.
macro_rules! tuple_runtime {
    ($($s:ident),+) => {
        #[allow(non_snake_case)]
        impl<P, $($s: Spore),+> SporeRuntime<P> for ($($s,)+) {
            fn find(&self, name: &str) -> Option<&dyn Any> {
                let ($($s,)+) = self;
                $(
                    if $s::NAME == name {
                        return Some($s);
                    }
                )+
                None
            }

            fn settings_schema(&self, key: &AppKey) -> Option<SettingsSchema> {
                $(
                    if is::<$s>(key) {
                        return $s::settings_schema().and_then(|s| SettingsSchema::parse(s).ok());
                    }
                )+
                None
            }

            fn check_settings(
                &self,
                key: &AppKey,
                settings: &serde_json::Map<String, serde_json::Value>,
            ) -> Result<(), Vec<SettingsProblem>> {
                let ($($s,)+) = self;
                $(
                    if is::<$s>(key) {
                        return $s.check_settings(settings);
                    }
                )+
                Ok(())
            }
        }
    };
}

tuple_runtime!(A);
tuple_runtime!(A, B);
tuple_runtime!(A, B, C);
tuple_runtime!(A, B, C, D);
tuple_runtime!(A, B, C, D, E);
tuple_runtime!(A, B, C, D, E, F);
tuple_runtime!(A, B, C, D, E, F, G);
tuple_runtime!(A, B, C, D, E, F, G, H);

/// What `SporeSet::build` makes: the spores themselves (the set's own type, a tuple of
/// built spores), and what they contribute, each one set.
pub struct Built<S: SporeSet> {
    pub spores: S,
    pub handlers: S::Handlers,
    pub payment_providers: S::PaymentProviders,
}

/// The spores a host is built with: `()` for none, or a tuple of `Spore`s, which
/// `host! { spores: [A, B] }` writes as `(A, B)`; built, the same tuple holds the
/// spores. Their providers and handlers are each one set (`Union`), joined to the
/// host's own.
pub trait SporeSet: Sized + Send + Sync + 'static {
    /// Every spore's handlers, in order.
    type Handlers: Send + Sync + 'static;
    /// Every spore's payment providers, in order.
    type PaymentProviders: PaymentProviders + Send + Sync + 'static;

    /// What the host registers in `apps` at startup (design note 0030, §3).
    fn registrations() -> Vec<SporeRegistration>;
    /// Their migrations, in order, to apply after core's.
    fn migrations() -> Vec<MigrationSet>;
    /// Each spore's name and settings schema as written, in order.
    fn settings_schemas() -> Vec<(&'static str, Option<serde_json::Value>)>;
    /// Builds each spore once, in order, with its environment from `host` and its
    /// section of `config` (`[spores.<name>]`), and takes what it contributes. The first
    /// spore that fails stops it.
    fn build(
        host: &SporeHost,
        config: &BTreeMap<String, toml::Table>,
    ) -> Result<Built<Self>, SporeStartError>;
    /// What's wrong with the providers `build` gave: a key that isn't its spore's
    /// (`spore_<NAME>` or `spore_<NAME>_…`), which payments would never route to it.
    fn check_providers(providers: &Self::PaymentProviders) -> Vec<String>;
}

/// Builds the spore `S` with its section of `config`.
fn build_one<S: Spore>(
    host: &SporeHost,
    config: &BTreeMap<String, toml::Table>,
) -> Result<S, SporeStartError> {
    let section = config.get(S::NAME).cloned().unwrap_or_default();
    S::new(host.env(S::NAME, section)).map_err(|error| SporeStartError {
        spore: S::NAME,
        error,
    })
}

impl SporeSet for () {
    type Handlers = ();
    type PaymentProviders = ();

    fn registrations() -> Vec<SporeRegistration> {
        Vec::new()
    }

    fn migrations() -> Vec<MigrationSet> {
        Vec::new()
    }

    fn settings_schemas() -> Vec<(&'static str, Option<serde_json::Value>)> {
        Vec::new()
    }

    fn build(
        _: &SporeHost,
        _: &BTreeMap<String, toml::Table>,
    ) -> Result<Built<()>, SporeStartError> {
        Ok(Built {
            spores: (),
            handlers: (),
            payment_providers: (),
        })
    }

    fn check_providers(_: &()) -> Vec<String> {
        Vec::new()
    }
}

/// `SporeSet` for tuples of 1 to 8 spores. The type parameters double as the names of
/// the built spores.
macro_rules! tuple_spores {
    ($($s:ident),+) => {
        #[allow(non_snake_case)]
        impl<$($s: Spore),+> SporeSet for ($($s,)+) {
            type Handlers = Union<($($s::Handlers,)+)>;
            type PaymentProviders = Union<($($s::PaymentProviders,)+)>;

            fn registrations() -> Vec<SporeRegistration> {
                vec![$(SporeRegistration {
                    name: $s::NAME,
                    title: $s::title(),
                    scopes: $s::SCOPES,
                }),+]
            }

            fn migrations() -> Vec<MigrationSet> {
                vec![$($s::migrations()),+]
            }

            fn settings_schemas() -> Vec<(&'static str, Option<serde_json::Value>)> {
                vec![$(($s::NAME, $s::settings_schema())),+]
            }

            fn build(
                host: &SporeHost,
                config: &BTreeMap<String, toml::Table>,
            ) -> Result<Built<Self>, SporeStartError> {
                $(let $s = build_one::<$s>(host, config)?;)+
                Ok(Built {
                    handlers: Union(($($s.handlers(),)+)),
                    payment_providers: Union(($($s.payment_providers(),)+)),
                    spores: ($($s,)+),
                })
            }

            fn check_providers(providers: &Self::PaymentProviders) -> Vec<String> {
                let Union(($($s,)+)) = providers;
                let mut problems = Vec::new();
                $(problems.extend(foreign_keys($s::NAME, &$s.keys()));)+
                problems
            }
        }
    };
}

tuple_spores!(A);
tuple_spores!(A, B);
tuple_spores!(A, B, C);
tuple_spores!(A, B, C, D);
tuple_spores!(A, B, C, D, E);
tuple_spores!(A, B, C, D, E, F);
tuple_spores!(A, B, C, D, E, F, G);
tuple_spores!(A, B, C, D, E, F, G, H);

/// A line for each of `keys` that isn't the spore `name`'s.
fn foreign_keys(name: &str, keys: &[ProviderKey]) -> Vec<String> {
    let Ok(owner) = AppKey::of_spore(name) else {
        // A bad name is `check`'s to report.
        return Vec::new();
    };
    keys.iter()
        .filter(|key| !key.belongs_to(&owner))
        .map(|key| format!("spore {name:?}: payment provider {key} isn't {owner} or {owner}_…"))
        .collect()
}

/// What's wrong with `hypha.config.toml`'s `[spores.*]` sections for a host built with
/// `S`: a section for a spore this build doesn't have (a typo, or a spore removed from
/// the build). Empty when all is well; the host refuses to start otherwise.
pub fn check_config<S: SporeSet>(config: &BTreeMap<String, toml::Table>) -> Vec<String> {
    let names: Vec<&str> = S::registrations().iter().map(|s| s.name).collect();
    config
        .keys()
        .filter(|name| !names.contains(&name.as_str()))
        .map(|name| format!("[spores.{name}]: this build has no spore {name:?}"))
        .collect()
}

/// What's wrong with a host's spores, every problem: a name that can't be a key
/// (`spore_<name>`), the same name twice, a name that extends another's with `_`
/// (`stripe` and `stripe_link`: the first's `spore_stripe_*` tables and provider keys
/// would take the second's), migrations under another module than `spore_<name>`. Empty
/// when all is well; the host refuses to start otherwise.
pub fn check<S: SporeSet>() -> Vec<String> {
    let mut problems = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for spore in S::registrations() {
        if AppKey::of_spore(spore.name).is_err() {
            problems.push(format!(
                "spore {:?}: not a valid name (lowercase letters, digits and _, starting with a letter)",
                spore.name
            ));
        }
        if seen.contains(&spore.name) {
            problems.push(format!("spore {:?}: listed twice", spore.name));
        }
        for other in &seen {
            let (shorter, longer) = match spore.name.len() < other.len() {
                true => (spore.name, *other),
                false => (*other, spore.name),
            };
            if longer
                .strip_prefix(shorter)
                .is_some_and(|rest| rest.starts_with('_'))
            {
                problems.push(format!(
                    "spore {longer:?}: its name extends spore {shorter:?}'s, whose tables and provider keys (spore_{shorter}_…) it would share"
                ));
            }
        }
        seen.push(spore.name);
    }
    for (name, schema) in S::settings_schemas() {
        if let Some(Err(errors)) = schema.map(SettingsSchema::parse) {
            for e in errors {
                problems.push(format!("spore {name:?}: its settings schema: {e}"));
            }
        }
    }
    for (spore, set) in S::registrations().iter().zip(S::migrations()) {
        let module = format!("spore_{}", spore.name);
        if set.module != module {
            problems.push(format!(
                "spore {:?}: its migrations are module {:?}, not {module:?}",
                spore.name, set.module
            ));
        }
    }
    problems
}

/// What's wrong with a host once its sets are built, every problem: a handler name or
/// provider key twice (deliveries and payment methods find them by it, so two alike
/// would be ambiguous: the first in the union would take both), or a spore provider's
/// key that isn't its spore's. Both hosts refuse to start (or serve) otherwise.
pub fn check_built<S: SporeSet>(
    handler_names: &[&str],
    provider_keys: &[ProviderKey],
    spore_providers: &S::PaymentProviders,
) -> Vec<String> {
    let mut problems = repeated("event handler", handler_names);
    problems.extend(repeated("payment provider", provider_keys));
    problems.extend(S::check_providers(spore_providers));
    problems
}

/// A line for each item that's in `list` more than once.
fn repeated<T: PartialEq + std::fmt::Display>(what: &str, list: &[T]) -> Vec<String> {
    list.iter()
        .enumerate()
        .filter(|(i, item)| list[..*i].contains(item))
        .map(|(_, item)| format!("{what} {item} is built in twice"))
        .collect()
}

#[cfg(test)]
mod tests {
    use hypha_payments::{PaymentError, PaymentProvider, PaymentRequest, ProviderKey, Started};

    use super::*;

    struct Pay;

    impl PaymentProvider for Pay {
        fn key(&self) -> ProviderKey {
            ProviderKey::new("spore_pay").unwrap()
        }

        fn check_settings(&self, _: &serde_json::Value) -> Result<(), PaymentError> {
            Ok(())
        }

        async fn start(&self, _: &PaymentRequest) -> Result<Started, PaymentError> {
            Err(PaymentError::UnknownProvider(self.key()))
        }
    }

    struct Paying;

    impl Spore for Paying {
        const NAME: &'static str = "pay";
        const SCOPES: &'static [Scope] = &[Scope::PaymentsWrite];

        fn title() -> &'static str {
            "Pay"
        }

        fn migrations() -> MigrationSet {
            MigrationSet {
                module: "spore_pay",
                migrations: &[],
            }
        }

        fn new(env: SporeEnv) -> Result<Self, SporeError> {
            match env.config().get("refuse") {
                Some(_) => Err(SporeError::Config {
                    key: Some("refuse".to_string()),
                    message: "refused".to_string(),
                }),
                None => Ok(Paying),
            }
        }

        type PaymentProviders = (Pay,);
        type Handlers = ();

        fn payment_providers(&self) -> (Pay,) {
            (Pay,)
        }

        fn handlers(&self) {}
    }

    /// A spore whose name and migrations are wrong.
    struct Broken;

    impl Spore for Broken {
        const NAME: &'static str = "Broken";
        const SCOPES: &'static [Scope] = &[];

        fn title() -> &'static str {
            "Broken"
        }

        fn migrations() -> MigrationSet {
            MigrationSet {
                module: "core",
                migrations: &[],
            }
        }

        fn new(_: SporeEnv) -> Result<Self, SporeError> {
            Ok(Broken)
        }

        type PaymentProviders = ();
        type Handlers = ();

        fn payment_providers(&self) {}

        fn handlers(&self) {}
    }

    #[test]
    fn no_spores_add_nothing() {
        assert!(<() as SporeSet>::registrations().is_empty());
        assert!(<() as SporeSet>::migrations().is_empty());
        assert!(check::<()>().is_empty());
    }

    #[test]
    fn a_set_registers_each_spore_with_its_scopes_and_joins_its_providers() {
        type Set = (Paying,);
        assert_eq!(
            Set::registrations(),
            [SporeRegistration {
                name: "pay",
                title: "Pay",
                scopes: &[Scope::PaymentsWrite],
            }]
        );
        let migrations = Set::migrations();
        assert_eq!(migrations[0].module, "spore_pay");
        assert!(check::<Set>().is_empty());
        let built = Set::build(&env::testing::host(), &BTreeMap::new()).unwrap();
        assert_eq!(
            built.payment_providers.keys(),
            [ProviderKey::new("spore_pay").unwrap()]
        );
    }

    #[test]
    fn a_spore_that_fails_to_build_stops_the_set_naming_it() {
        let config = BTreeMap::from([("pay".to_string(), "refuse = true".parse().unwrap())]);
        let error = <(Paying,)>::build(&env::testing::host(), &config).err();
        assert_eq!(
            error.map(|e| e.to_string()),
            Some("spore \"pay\": refuse: refused".to_string())
        );
    }

    #[test]
    fn a_section_for_a_spore_the_build_lacks_is_reported() {
        let config = BTreeMap::from([
            ("pay".to_string(), toml::Table::new()),
            ("stripe".to_string(), toml::Table::new()),
        ]);
        assert_eq!(
            check_config::<(Paying,)>(&config),
            ["[spores.stripe]: this build has no spore \"stripe\""]
        );
        assert!(check_config::<(Paying,)>(&BTreeMap::new()).is_empty());
    }

    #[test]
    fn a_bad_name_a_repeat_and_a_wrong_module_are_all_reported() {
        assert_eq!(
            check::<(Paying, Paying, Broken)>(),
            [
                "spore \"pay\": listed twice",
                "spore \"Broken\": not a valid name (lowercase letters, digits and _, starting with a letter)",
                "spore \"Broken\": its migrations are module \"core\", not \"spore_Broken\"",
            ]
        );
    }

    /// A spore named `pay_link`, with no providers.
    struct PayLink;

    impl Spore for PayLink {
        const NAME: &'static str = "pay_link";
        const SCOPES: &'static [Scope] = &[];

        fn title() -> &'static str {
            "Pay link"
        }

        fn migrations() -> MigrationSet {
            MigrationSet {
                module: "spore_pay_link",
                migrations: &[],
            }
        }

        fn new(_: SporeEnv) -> Result<Self, SporeError> {
            Ok(PayLink)
        }

        type PaymentProviders = ();
        type Handlers = ();

        fn payment_providers(&self) {}

        fn handlers(&self) {}
    }

    /// A spore that offers another spore's provider key.
    struct Rogue;

    impl Spore for Rogue {
        const NAME: &'static str = "rogue";
        const SCOPES: &'static [Scope] = &[];

        fn title() -> &'static str {
            "Rogue"
        }

        fn migrations() -> MigrationSet {
            MigrationSet {
                module: "spore_rogue",
                migrations: &[],
            }
        }

        fn new(_: SporeEnv) -> Result<Self, SporeError> {
            Ok(Rogue)
        }

        type PaymentProviders = (Pay,);
        type Handlers = ();

        fn payment_providers(&self) -> (Pay,) {
            (Pay,)
        }

        fn handlers(&self) {}
    }

    #[test]
    fn a_name_extending_another_is_reported_whichever_comes_first() {
        let problem = "spore \"pay_link\": its name extends spore \"pay\"'s, whose tables and provider keys (spore_pay_…) it would share";
        assert_eq!(check::<(Paying, PayLink)>(), [problem]);
        assert_eq!(check::<(PayLink, Paying)>(), [problem]);
    }

    #[test]
    fn a_provider_under_another_spores_key_is_reported() {
        type Set = (Rogue,);
        let providers = Set::build(&env::testing::host(), &BTreeMap::new())
            .unwrap()
            .payment_providers;
        assert_eq!(
            Set::check_providers(&providers),
            ["spore \"rogue\": payment provider spore_pay isn't spore_rogue or spore_rogue_…"]
        );
        let providers = <(Paying,)>::build(&env::testing::host(), &BTreeMap::new())
            .unwrap()
            .payment_providers;
        assert!(<(Paying,)>::check_providers(&providers).is_empty());
    }

    #[test]
    fn each_repeat_is_reported_once_per_extra_copy() {
        assert_eq!(
            repeated(
                "event handler",
                &["search_index", "mailer", "search_index", "search_index"]
            ),
            [
                "event handler search_index is built in twice",
                "event handler search_index is built in twice",
            ]
        );
        assert!(repeated("payment provider", &["manual", "spore_stripe"]).is_empty());
    }

    #[test]
    fn a_built_host_reports_repeated_names_and_keys() {
        let providers = <(Paying,)>::build(&env::testing::host(), &BTreeMap::new())
            .unwrap()
            .payment_providers;
        let manual = ProviderKey::manual();
        assert_eq!(
            check_built::<(Paying,)>(
                &["search_index", "spore_pay_audit", "search_index"],
                &[manual.clone(), manual],
                &providers,
            ),
            [
                "event handler search_index is built in twice",
                "payment provider manual is built in twice",
            ]
        );
    }
}
