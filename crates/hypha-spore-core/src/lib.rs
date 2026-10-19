//! Spores (design note 0030): trusted Rust crates compiled into the API beside the
//! external apps of design note 0029. This crate is the `Spore` trait a spore implements
//! and `SporeSet`, the list of spores a host is built with (`host! { spores: [...] }`).
//!
//! It's the minimum the hosts need (step 2 of 0030's build order): what a spore is
//! called, the scopes it acts with, its tables, and what it adds to the host's payment
//! providers and event handlers. The `hypha-spore` facade (step 3) re-exports it with
//! everything else a spore may use; spores should depend on that, once it exists.

use hypha_apps::SporeRegistration;
use hypha_core::{AppKey, Scope, Union};
use hypha_payments::{PaymentProviders, ProviderKey};
use hypha_store_postgres::MigrationSet;

/// One spore. Built once at startup; everything it contributes goes into the host's
/// tuples (static dispatch, so `Send` stays provable on the native host).
pub trait Spore: Sized + 'static {
    /// `stripe`: its app key (`spore_stripe`, design note 0030, §3), its migration module
    /// and table prefix (`spore_stripe`, `spore_stripe_*`). A contract: never rename one
    /// that has shipped.
    const NAME: &'static str;
    /// What it may do, as `Actor::App` (SP2): its grant, shown to staff.
    const SCOPES: &'static [Scope];

    /// Its title in the admin ("Stripe").
    fn title() -> &'static str;

    /// Its own tables (SP4): module `spore_<NAME>` (the host checks), applied after
    /// core's; `MigrationSet { module: "spore_<NAME>", migrations: &[] }` for none.
    fn migrations() -> MigrationSet;

    /// Builds it, at startup. (The host's environment, its configuration section and
    /// secrets come with step 3: design note 0030, §4 and §8.)
    fn new() -> Self;

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

/// The spores a host is built with: `()` for none, or a tuple of `Spore`s, which
/// `host! { spores: [A, B] }` writes as `(A, B)`. Their providers and handlers are each
/// one set (`Union`), joined to the host's own.
pub trait SporeSet: 'static {
    /// Every spore's handlers, in order.
    type Handlers: Send + Sync + 'static;
    /// Every spore's payment providers, in order.
    type PaymentProviders: PaymentProviders + Send + Sync + 'static;

    /// What the host registers in `apps` at startup (design note 0030, §3).
    fn registrations() -> Vec<SporeRegistration>;
    /// Their migrations, in order, to apply after core's.
    fn migrations() -> Vec<MigrationSet>;
    /// Builds each spore once and takes what it contributes.
    fn build() -> (Self::Handlers, Self::PaymentProviders);
    /// What's wrong with the providers `build` gave: a key that isn't its spore's
    /// (`spore_<NAME>` or `spore_<NAME>_…`), which payments would never route to it.
    fn check_providers(providers: &Self::PaymentProviders) -> Vec<String>;
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

    fn build() -> ((), ()) {
        ((), ())
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

            fn build() -> (Self::Handlers, Self::PaymentProviders) {
                $(let $s = $s::new();)+
                (
                    Union(($($s.handlers(),)+)),
                    Union(($($s.payment_providers(),)+)),
                )
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

        fn new() -> Self {
            Paying
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

        fn new() -> Self {
            Broken
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
        let (_, providers) = Set::build();
        assert_eq!(providers.keys(), [ProviderKey::new("spore_pay").unwrap()]);
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

        fn new() -> Self {
            PayLink
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

        fn new() -> Self {
            Rogue
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
        let (_, providers) = Set::build();
        assert_eq!(
            Set::check_providers(&providers),
            ["spore \"rogue\": payment provider spore_pay isn't spore_rogue or spore_rogue_…"]
        );
        let (_, providers) = <(Paying,)>::build();
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
        let (_, providers) = <(Paying,)>::build();
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
