use hypha_core::{
    OrderId, PaymentId, PaymentMethodId, Recorded, RepoError, Versioned, stamp_insert, stamp_update,
};
use hypha_payments::{Payment, PaymentMethod, PaymentMethodRepo, PaymentRepo, PaymentStatus, keys};

use crate::{
    InMemoryOutbox,
    table::{Table, no_unique_keys},
};

#[derive(Default)]
pub struct InMemoryPaymentMethodRepo {
    pub(crate) methods: Table<PaymentMethodId, PaymentMethod>,
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
        rows.remove(&method.id());
        Ok(())
    }
}

#[derive(Default)]
pub struct InMemoryPaymentRepo {
    pub(crate) payments: Table<PaymentId, Payment>,
    pub(crate) outbox: InMemoryOutbox,
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
}

impl PaymentRepo for InMemoryPaymentRepo {
    async fn get(&self, id: PaymentId) -> Result<Option<Payment>, RepoError> {
        Ok(self.payments.get(&id))
    }

    async fn list_for_order(&self, order: OrderId) -> Result<Vec<Payment>, RepoError> {
        let mut found: Vec<Payment> = self
            .payments
            .all()
            .into_iter()
            .filter(|p| p.order() == order)
            .collect();
        found.sort_by_key(|p| std::cmp::Reverse(p.id().as_uuid()));
        Ok(found)
    }

    async fn pending_for_method(&self, method: PaymentMethodId) -> Result<bool, RepoError> {
        Ok(self
            .payments
            .find(|p| p.method() == Some(method) && p.status() == PaymentStatus::Pending)
            .is_some())
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
