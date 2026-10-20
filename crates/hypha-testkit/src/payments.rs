use std::sync::{Arc, Mutex};

use hypha_core::{
    CartId, OrderId, PaymentId, PaymentMethodId, Recorded, RepoError, Versioned, stamp_insert,
    stamp_update,
};
use hypha_payments::{Payment, PaymentMethod, PaymentMethodRepo, PaymentRepo, keys};

use crate::{
    InMemoryOutbox,
    table::{Table, no_unique_keys},
};

#[derive(Default)]
pub struct InMemoryPaymentMethodRepo {
    pub(crate) methods: Table<PaymentMethodId, PaymentMethod>,
    /// The payments, when linked (`linked`, as `InMemoryPorts` does): a method an open
    /// payment uses can't be deleted. Made on its own, it checks nothing.
    pub(crate) payments: Option<Arc<Table<PaymentId, Payment>>>,
    /// Shared with `InMemoryPorts`' other repos: the clock writes are stamped with
    /// (methods record no events).
    pub(crate) outbox: InMemoryOutbox,
}

impl InMemoryPaymentMethodRepo {
    /// A repo stamping its writes with `outbox`'s clock (`InMemoryPorts` shares one).
    pub fn sharing(outbox: &InMemoryOutbox) -> Self {
        Self {
            outbox: outbox.clone(),
            ..Self::default()
        }
    }
}

impl PaymentMethodRepo for InMemoryPaymentMethodRepo {
    async fn get(&self, id: PaymentMethodId) -> Result<Option<PaymentMethod>, RepoError> {
        Ok(self.methods.get(&id))
    }

    async fn list(&self) -> Result<Vec<PaymentMethod>, RepoError> {
        let mut all = self.methods.all();
        all.sort_by_key(|m| (m.position(), m.id()));
        Ok(all)
    }

    async fn insert(&self, method: &mut PaymentMethod) -> Result<(), RepoError> {
        stamp_insert(method, self.outbox.now());
        self.methods.insert(
            method.id(),
            method,
            keys::PAYMENT_METHOD_ID,
            &no_unique_keys,
        )
    }

    async fn update(&self, method: &mut PaymentMethod) -> Result<(), RepoError> {
        stamp_update(method, self.outbox.now());
        self.methods.update(method.id(), method, &no_unique_keys)
    }

    async fn delete(&self, method: &PaymentMethod) -> Result<(), RepoError> {
        let mut rows = self.methods.lock();
        match rows.get(&method.id()) {
            Some(stored) if stored.version() == method.version() => {}
            _ => return Err(RepoError::StaleVersion),
        }
        // Checked while the methods are locked, like the database's one statement.
        if let Some(payments) = &self.payments
            && payments
                .lock()
                .values()
                .any(|p| p.method() == Some(method.id()) && p.status().is_open())
        {
            return Err(RepoError::Reference(keys::OPEN_PAYMENT_METHOD));
        }
        rows.remove(&method.id());
        Ok(())
    }
}

#[derive(Default)]
pub struct InMemoryPaymentRepo {
    pub(crate) payments: Arc<Table<PaymentId, Payment>>,
    pub(crate) outbox: InMemoryOutbox,
    /// What the next `list_for_order` returns instead (`serve_next_list`).
    stale: Mutex<Option<Vec<Payment>>>,
}

impl InMemoryPaymentRepo {
    /// A repo writing its events to `outbox` (`InMemoryPorts` shares one).
    pub fn sharing(outbox: &InMemoryOutbox) -> Self {
        Self {
            outbox: outbox.clone(),
            ..Self::default()
        }
    }

    /// The events this repo wrote (into its outbox, shared or not).
    pub fn outbox(&self) -> &InMemoryOutbox {
        &self.outbox
    }

    /// The next `list_for_order` returns `payments`, copies loaded before another
    /// request committed: for testing a command that loses that race.
    pub fn serve_next_list(&self, payments: Vec<Payment>) {
        *self.stale.lock().unwrap() = Some(payments);
    }
}

impl PaymentRepo for InMemoryPaymentRepo {
    async fn get(&self, id: PaymentId) -> Result<Option<Payment>, RepoError> {
        Ok(self.payments.get(&id))
    }

    async fn list_for_order(&self, order: OrderId) -> Result<Vec<Payment>, RepoError> {
        let stale = self.stale.lock().unwrap().take();
        if let Some(payments) = stale {
            return Ok(payments);
        }
        let mut found: Vec<Payment> = self
            .payments
            .all()
            .into_iter()
            .filter(|p| p.order() == Some(order))
            .collect();
        found.sort_by_key(|p| std::cmp::Reverse(p.id().as_uuid()));
        Ok(found)
    }

    async fn list_for_cart(&self, cart: CartId) -> Result<Vec<Payment>, RepoError> {
        let mut found: Vec<Payment> = self
            .payments
            .all()
            .into_iter()
            .filter(|p| p.cart() == Some(cart))
            .collect();
        found.sort_by_key(|p| std::cmp::Reverse(p.id().as_uuid()));
        Ok(found)
    }

    async fn insert(&self, payment: &mut Payment) -> Result<(), RepoError> {
        stamp_insert(payment, self.outbox.now());
        let events = payment.take_events();
        self.payments
            .insert(payment.id(), payment, keys::PAYMENT_ID, &no_unique_keys)?;
        self.outbox.write(events, Some(payment.version()));
        Ok(())
    }

    async fn update(&self, payment: &mut Payment) -> Result<(), RepoError> {
        stamp_update(payment, self.outbox.now());
        let events = payment.take_events();
        self.payments
            .update(payment.id(), payment, &no_unique_keys)?;
        self.outbox.write(events, Some(payment.version()));
        Ok(())
    }
}

/// A method repo and a payment repo linked like the database's tables (deleting a
/// method checks its open payments), sharing `outbox`.
pub(crate) fn linked(outbox: &InMemoryOutbox) -> (InMemoryPaymentMethodRepo, InMemoryPaymentRepo) {
    let payments = InMemoryPaymentRepo::sharing(outbox);
    let methods = InMemoryPaymentMethodRepo {
        payments: Some(Arc::clone(&payments.payments)),
        ..InMemoryPaymentMethodRepo::sharing(outbox)
    };
    (methods, payments)
}
