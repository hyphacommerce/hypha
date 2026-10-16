//! In-process event handlers (design note 0019, section 5; 0002 §6): destinations in
//! code for domain events, next to webhooks to apps. The dispatcher runs whatever set
//! the host gives it, so a domain crate or a spore adds a handler without the dispatcher
//! knowing it, and each handler asks for just the repositories it uses.
//!
//! A set is a tuple of handlers, `(VariantCleanup, SearchIndexer)`: static dispatch, so
//! a host on concrete ports can still prove the futures `Send` (a `dyn` handler couldn't
//! have a generic `async fn`).

use crate::StoredEvent;

/// One handler. Deliveries to it are stored as `handler:<name>`, so `name` is a
/// contract: never rename one that has shipped.
#[allow(async_fn_in_trait)]
pub trait EventHandler<P: ?Sized> {
    /// Lowercase snake case, unique in the host's set ("variant_cleanup").
    fn name(&self) -> &'static str;
    /// Whether it wants events of `event_type`; read when events are fanned out.
    fn wants(&self, event_type: &str) -> bool;
    /// Handles `event`. Delivery is at least once (a lease can run out mid-run), so
    /// running it twice must do no harm. `Err` with why: it's tried again on the retry
    /// schedule, and the admin shows the message.
    async fn handle(&self, ports: &P, event: &StoredEvent) -> Result<(), String>;
}

/// The set of handlers a host runs: `()` for none, or a tuple of `EventHandler`s.
#[allow(async_fn_in_trait)]
pub trait EventHandlers<P: ?Sized> {
    /// Every handler's name, in order.
    fn names(&self) -> Vec<&'static str>;
    /// The names of the handlers that want events of `event_type`.
    fn wanting(&self, event_type: &str) -> Vec<&'static str>;
    /// Runs the handler called `name`; `None` if the set has none (it was removed from
    /// the build while deliveries to it were pending).
    async fn run(&self, name: &str, ports: &P, event: &StoredEvent) -> Option<Result<(), String>>;
}

impl<P: ?Sized> EventHandlers<P> for () {
    fn names(&self) -> Vec<&'static str> {
        Vec::new()
    }

    fn wanting(&self, _event_type: &str) -> Vec<&'static str> {
        Vec::new()
    }

    async fn run(&self, _: &str, _: &P, _: &StoredEvent) -> Option<Result<(), String>> {
        None
    }
}

/// `EventHandlers` for tuples of 1 to 12 handlers. The type parameters double as the
/// names of the tuple's fields when it's destructured.
macro_rules! tuple_handlers {
    ($($h:ident),+) => {
        #[allow(non_snake_case)]
        impl<P: ?Sized, $($h: EventHandler<P>),+> EventHandlers<P> for ($($h,)+) {
            fn names(&self) -> Vec<&'static str> {
                let ($($h,)+) = self;
                vec![$($h.name()),+]
            }

            fn wanting(&self, event_type: &str) -> Vec<&'static str> {
                let ($($h,)+) = self;
                let mut names = Vec::new();
                $(
                    if $h.wants(event_type) {
                        names.push($h.name());
                    }
                )+
                names
            }

            async fn run(
                &self,
                name: &str,
                ports: &P,
                event: &StoredEvent,
            ) -> Option<Result<(), String>> {
                let ($($h,)+) = self;
                $(
                    if $h.name() == name {
                        return Some($h.handle(ports, event).await);
                    }
                )+
                None
            }
        }
    };
}

tuple_handlers!(A);
tuple_handlers!(A, B);
tuple_handlers!(A, B, C);
tuple_handlers!(A, B, C, D);
tuple_handlers!(A, B, C, D, E);
tuple_handlers!(A, B, C, D, E, F);
tuple_handlers!(A, B, C, D, E, F, G);
tuple_handlers!(A, B, C, D, E, F, G, H);
tuple_handlers!(A, B, C, D, E, F, G, H, I);
tuple_handlers!(A, B, C, D, E, F, G, H, I, J);
tuple_handlers!(A, B, C, D, E, F, G, H, I, J, K);
tuple_handlers!(A, B, C, D, E, F, G, H, I, J, K, L);

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use uuid::Uuid;

    use super::*;
    use crate::{Actor, Timestamp};

    /// Records what it handled in the ports, a list of strings.
    struct Named(&'static str, &'static [&'static str]);

    impl EventHandler<RefCell<Vec<String>>> for Named {
        fn name(&self) -> &'static str {
            self.0
        }

        fn wants(&self, event_type: &str) -> bool {
            self.1.contains(&event_type)
        }

        async fn handle(
            &self,
            ports: &RefCell<Vec<String>>,
            event: &StoredEvent,
        ) -> Result<(), String> {
            ports
                .borrow_mut()
                .push(format!("{} {}", self.0, event.event_type));
            Ok(())
        }
    }

    fn event(event_type: &str) -> StoredEvent {
        StoredEvent {
            seq: 1,
            id: Uuid::from_u128(1),
            event_type: event_type.to_string(),
            version: 1,
            subject_kind: "product".to_string(),
            subject_id: Uuid::from_u128(2),
            subject_version: None,
            actor: Actor::System,
            occurred_at: Timestamp::from_unix_micros(0),
            data: serde_json::Value::Null,
        }
    }

    fn set() -> (Named, Named) {
        (
            Named("cleanup", &["product.variant_removed"]),
            Named("search", &["product.updated", "product.variant_removed"]),
        )
    }

    #[test]
    fn names_are_in_order() {
        assert_eq!(
            EventHandlers::<RefCell<Vec<String>>>::names(&set()),
            ["cleanup", "search"]
        );
    }

    #[test]
    fn wanting_lists_the_handlers_for_a_type() {
        let set = set();
        let wanting = |t| EventHandlers::<RefCell<Vec<String>>>::wanting(&set, t);
        assert_eq!(wanting("product.variant_removed"), ["cleanup", "search"]);
        assert_eq!(wanting("product.updated"), ["search"]);
        assert!(wanting("order.placed").is_empty());
    }

    #[test]
    fn run_runs_the_named_handler_only() {
        let ports = RefCell::new(Vec::new());
        let ran = pollster::block_on(set().run("search", &ports, &event("product.updated")));
        assert_eq!(ran, Some(Ok(())));
        assert_eq!(ports.into_inner(), ["search product.updated"]);
    }

    #[test]
    fn an_unknown_name_runs_nothing() {
        let ports = RefCell::new(Vec::new());
        let ran = pollster::block_on(set().run("gone", &ports, &event("product.updated")));
        assert_eq!(ran, None);
        assert!(ports.borrow().is_empty());
        assert_eq!(
            pollster::block_on(().run("gone", &ports, &event("x"))),
            None
        );
    }
}
