use hypha_checkout_rules::{CheckoutRule, CheckoutRuleRepo, keys};
use hypha_core::{CheckoutRuleId, Recorded, RepoError, Versioned, stamp_insert, stamp_update};

use crate::{
    InMemoryOutbox,
    table::{Table, no_unique_keys},
};

/// The store's checkout rules.
#[derive(Default)]
pub struct InMemoryCheckoutRules {
    rules: Table<CheckoutRuleId, CheckoutRule>,
    outbox: InMemoryOutbox,
}

impl InMemoryCheckoutRules {
    /// A repo writing its events to `outbox` (`InMemoryPorts` shares one).
    pub fn sharing(outbox: &InMemoryOutbox) -> Self {
        Self {
            outbox: outbox.clone(),
            ..Self::default()
        }
    }
}

impl CheckoutRuleRepo for InMemoryCheckoutRules {
    async fn get(&self, id: CheckoutRuleId) -> Result<Option<CheckoutRule>, RepoError> {
        Ok(self.rules.get(&id))
    }

    async fn list(&self) -> Result<Vec<CheckoutRule>, RepoError> {
        let mut rules = self.rules.all();
        rules.sort_by_key(|r| (r.position(), r.id()));
        Ok(rules)
    }

    async fn insert(&self, rule: &mut CheckoutRule) -> Result<(), RepoError> {
        stamp_insert(rule, self.outbox.now());
        let events = rule.take_events();
        self.rules
            .insert(rule.id(), rule, keys::CHECKOUT_RULE_ID, &no_unique_keys)?;
        self.outbox.write(events, Some(rule.version()));
        Ok(())
    }

    async fn update_many(&self, rules: &mut [CheckoutRule]) -> Result<(), RepoError> {
        let now = self.outbox.now();
        let events: Vec<_> = rules
            .iter_mut()
            .map(|rule| {
                stamp_update(rule, now);
                rule.take_events()
            })
            .collect();
        self.rules
            .update_many(rules, CheckoutRule::id, &no_unique_keys)?;
        for (rule, events) in rules.iter().zip(events) {
            self.outbox.write(events, Some(rule.version()));
        }
        Ok(())
    }

    async fn delete(&self, rule: &mut CheckoutRule) -> Result<(), RepoError> {
        let mut rows = self.rules.lock();
        match rows.get(&rule.id()) {
            Some(stored) if stored.version() == rule.version() => {}
            _ => return Err(RepoError::StaleVersion),
        }
        rows.remove(&rule.id());
        self.outbox.write(rule.take_events(), None);
        Ok(())
    }
}
