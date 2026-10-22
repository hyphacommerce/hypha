//! Payment methods and payments: one row each, no child tables.

use hypha_core::{
    CartId, ChannelId, MarketId, Money, OrderId, PaymentId, PaymentMethodId, Recorded, RepoError,
    Version, Versioned, stamp_insert, stamp_update,
};
use hypha_payments::{
    Payment, PaymentMethod, PaymentMethodRepo, PaymentMethodSnapshot, PaymentRepo, PaymentSnapshot,
    ProviderKey, keys,
};
use tokio_postgres::{Row, types::Type};
use uuid::Uuid;

use crate::{
    columns::{
        col, currency_from_db, micros, opt_micros, opt_ts, stamp_columns, stamps_from_row, ts,
        version_from_db, version_to_db, write_stamps,
    },
    error::{corrupt, repo_error},
    outbox::insert_events,
    store::Queries,
    store::{PgConnection, PgStore, in_transaction},
};

fn method_columns() -> String {
    format!(
        "id, version, provider, name, instructions, market_ids, channel_ids, enabled, position,
         settings::text AS settings, {}",
        stamp_columns("")
    )
}

fn method_from_row(row: &Row) -> Result<PaymentMethod, RepoError> {
    let id = PaymentMethodId::from_uuid(col(row, "id")?);
    let provider: String = col(row, "provider")?;
    let markets: Vec<Uuid> = col(row, "market_ids")?;
    let channels: Vec<Uuid> = col(row, "channel_ids")?;
    let position: i64 = col(row, "position")?;
    let settings: String = col(row, "settings")?;
    PaymentMethod::restore(PaymentMethodSnapshot {
        id,
        version: version_from_db(col(row, "version")?)?,
        stamps: stamps_from_row(row)?,
        provider: ProviderKey::new(&provider).map_err(corrupt)?,
        name: col(row, "name")?,
        instructions: col(row, "instructions")?,
        markets: markets.into_iter().map(MarketId::from_uuid).collect(),
        channels: channels.into_iter().map(ChannelId::from_uuid).collect(),
        enabled: col(row, "enabled")?,
        position: u32::try_from(position)
            .map_err(|_| corrupt(format!("payment method {id} position {position}")))?,
        settings: serde_json::from_str(&settings)
            .map_err(|e| corrupt(format!("payment method {id} settings: {e}")))?,
    })
    .map_err(|e| corrupt(format!("payment method {id}: {e}")))
}

fn ids<T>(values: &[T], uuid: impl Fn(&T) -> Uuid) -> Vec<Uuid> {
    values.iter().map(uuid).collect()
}

impl<C: PgConnection> PaymentMethodRepo for PgStore<C> {
    async fn get(&self, id: PaymentMethodId) -> Result<Option<PaymentMethod>, RepoError> {
        let mut conn = self.conn().await;
        let row = conn
            .db()
            .query_typed_opt(
                &format!(
                    "SELECT {} FROM payment_methods WHERE id = $1",
                    method_columns()
                ),
                &[(&id.as_uuid(), Type::UUID)],
            )
            .await
            .map_err(repo_error)?;
        row.as_ref().map(method_from_row).transpose()
    }

    async fn list(&self) -> Result<Vec<PaymentMethod>, RepoError> {
        let mut conn = self.conn().await;
        conn.db()
            .query_typed(
                &format!(
                    "SELECT {} FROM payment_methods ORDER BY position, id",
                    method_columns()
                ),
                &[],
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(method_from_row)
            .collect()
    }

    async fn insert(&self, method: &mut PaymentMethod) -> Result<(), RepoError> {
        stamp_insert(method, self.now());
        let s = method.snapshot();
        let settings = s.settings.to_string();
        in_transaction!(self, |tx| {
            tx.query_typed(
                "INSERT INTO payment_methods (id, version, provider, name, instructions,
                     market_ids, channel_ids, enabled, position, settings)
                 VALUES ($1, 1, $2, $3, $4, $5, $6, $7, $8, $9::text::jsonb)",
                &[
                    (&s.id.as_uuid(), Type::UUID),
                    (&s.provider.as_str(), Type::TEXT),
                    (&s.name, Type::TEXT),
                    (&s.instructions, Type::TEXT),
                    (&ids(&s.markets, MarketId::as_uuid), Type::UUID_ARRAY),
                    (&ids(&s.channels, ChannelId::as_uuid), Type::UUID_ARRAY),
                    (&s.enabled, Type::BOOL),
                    (&i64::from(s.position), Type::INT8),
                    (&settings, Type::TEXT),
                ],
            )
            .await
            .map_err(repo_error)?;
            write_stamps(
                tx,
                "payment_methods",
                "id",
                &s.id.as_uuid(),
                Type::UUID,
                s.stamps,
            )
            .await
        })?;
        method.set_version(Version::NEW.next());
        Ok(())
    }

    async fn update(&self, method: &mut PaymentMethod) -> Result<(), RepoError> {
        stamp_update(method, self.now());
        let s = method.snapshot();
        let settings = s.settings.to_string();
        let version = in_transaction!(self, |tx| {
            let row = tx
                .query_typed_opt(
                    "UPDATE payment_methods SET version = version + 1, provider = $3, name = $4,
                         instructions = $5, market_ids = $6, channel_ids = $7, enabled = $8,
                         position = $9, settings = $10::text::jsonb
                     WHERE id = $1 AND version = $2 RETURNING version",
                    &[
                        (&s.id.as_uuid(), Type::UUID),
                        (&version_to_db(s.version)?, Type::INT8),
                        (&s.provider.as_str(), Type::TEXT),
                        (&s.name, Type::TEXT),
                        (&s.instructions, Type::TEXT),
                        (&ids(&s.markets, MarketId::as_uuid), Type::UUID_ARRAY),
                        (&ids(&s.channels, ChannelId::as_uuid), Type::UUID_ARRAY),
                        (&s.enabled, Type::BOOL),
                        (&i64::from(s.position), Type::INT8),
                        (&settings, Type::TEXT),
                    ],
                )
                .await
                .map_err(repo_error)?
                .ok_or(RepoError::StaleVersion)?;
            let version = version_from_db(col(&row, "version")?)?;
            write_stamps(
                tx,
                "payment_methods",
                "id",
                &s.id.as_uuid(),
                Type::UUID,
                s.stamps,
            )
            .await?;
            Ok(version)
        })?;
        method.set_version(version);
        Ok(())
    }

    async fn delete(&self, method: &PaymentMethod) -> Result<(), RepoError> {
        let id = method.id().as_uuid();
        let version = version_to_db(method.version())?;
        let mut conn = self.conn().await;
        // One statement: a checkout starting a payment with the method can't slip in
        // between the check and the delete.
        let deleted = conn
            .db()
            .query_typed_opt(
                "DELETE FROM payment_methods WHERE id = $1 AND version = $2
                     AND NOT EXISTS (SELECT 1 FROM payments WHERE method_id = $1
                                     AND status IN ('pending', 'authorized', 'processing'))
                 RETURNING id",
                &[(&id, Type::UUID), (&version, Type::INT8)],
            )
            .await
            .map_err(repo_error)?;
        if deleted.is_some() {
            return Ok(());
        }
        // Nothing deleted: still there at its version means an open payment held it.
        let held = conn
            .db()
            .query_typed_opt(
                "SELECT 1 AS found FROM payment_methods WHERE id = $1 AND version = $2",
                &[(&id, Type::UUID), (&version, Type::INT8)],
            )
            .await
            .map_err(repo_error)?;
        Err(match held {
            Some(_) => RepoError::Reference(keys::OPEN_PAYMENT_METHOD),
            None => RepoError::StaleVersion,
        })
    }
}

fn payment_columns() -> String {
    format!(
        "id, version, cart_id, order_id, method_id, method_name, provider, currency, amount, refunded,
         status, reference, refund_references, attempt, instructions, {}, {}, {}",
        micros("paid_at"),
        micros("voided_at"),
        stamp_columns("")
    )
}

fn payment_from_row(row: &Row) -> Result<Payment, RepoError> {
    let id = PaymentId::from_uuid(col(row, "id")?);
    let currency = currency_from_db(&col::<String>(row, "currency")?)?;
    let money = |column: &str| -> Result<Money, RepoError> {
        Money::new(col(row, column)?, currency).map_err(corrupt)
    };
    let provider: String = col(row, "provider")?;
    let status: String = col(row, "status")?;
    let method: Option<Uuid> = col(row, "method_id")?;
    let cart: Option<Uuid> = col(row, "cart_id")?;
    let order: Option<Uuid> = col(row, "order_id")?;
    Payment::restore(PaymentSnapshot {
        id,
        version: version_from_db(col(row, "version")?)?,
        stamps: stamps_from_row(row)?,
        cart: cart.map(CartId::from_uuid),
        order: order.map(OrderId::from_uuid),
        method: method.map(PaymentMethodId::from_uuid),
        method_name: col(row, "method_name")?,
        provider: ProviderKey::new(&provider).map_err(corrupt)?,
        amount: money("amount")?,
        status: status
            .parse()
            .map_err(|s| corrupt(format!("payment {id} status {s:?}")))?,
        refunded: money("refunded")?,
        reference: col(row, "reference")?,
        refund_references: col(row, "refund_references")?,
        attempt: col(row, "attempt")?,
        instructions: col(row, "instructions")?,
        paid_at: opt_ts(col(row, "paid_at")?),
        voided_at: opt_ts(col(row, "voided_at")?),
    })
    .map_err(|e| corrupt(format!("payment {id}: {e}")))
}

impl<C: PgConnection> PaymentRepo for PgStore<C> {
    async fn get(&self, id: PaymentId) -> Result<Option<Payment>, RepoError> {
        let mut conn = self.conn().await;
        let row = conn
            .db()
            .query_typed_opt(
                &format!("SELECT {} FROM payments WHERE id = $1", payment_columns()),
                &[(&id.as_uuid(), Type::UUID)],
            )
            .await
            .map_err(repo_error)?;
        row.as_ref().map(payment_from_row).transpose()
    }

    async fn list_for_order(&self, order: OrderId) -> Result<Vec<Payment>, RepoError> {
        let mut conn = self.conn().await;
        conn.db()
            .query_typed(
                &format!(
                    "SELECT {} FROM payments WHERE order_id = $1 ORDER BY id DESC",
                    payment_columns()
                ),
                &[(&order.as_uuid(), Type::UUID)],
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(payment_from_row)
            .collect()
    }

    async fn list_for_orders(&self, orders: &[OrderId]) -> Result<Vec<Payment>, RepoError> {
        if orders.is_empty() {
            return Ok(Vec::new());
        }
        let orders: Vec<uuid::Uuid> = orders.iter().map(|o| o.as_uuid()).collect();
        let mut conn = self.conn().await;
        conn.db()
            .query_typed(
                &format!(
                    "SELECT {} FROM payments WHERE order_id = ANY($1)
                     ORDER BY order_id, id DESC",
                    payment_columns()
                ),
                &[(&orders, Type::UUID_ARRAY)],
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(payment_from_row)
            .collect()
    }

    async fn list_for_cart(&self, cart: CartId) -> Result<Vec<Payment>, RepoError> {
        let mut conn = self.conn().await;
        conn.db()
            .query_typed(
                &format!(
                    "SELECT {} FROM payments WHERE cart_id = $1 ORDER BY id DESC",
                    payment_columns()
                ),
                &[(&cart.as_uuid(), Type::UUID)],
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(payment_from_row)
            .collect()
    }

    async fn insert(&self, payment: &mut Payment) -> Result<(), RepoError> {
        stamp_insert(payment, self.now());
        let s = payment.snapshot();
        let events = payment.take_events();
        let stamp = self.event_stamp();
        let version = Version::NEW.next();
        in_transaction!(self, |tx| {
            insert_payment(tx, &s).await?;
            insert_events(tx, &events, Some(version), stamp).await
        })?;
        payment.set_version(version);
        Ok(())
    }

    async fn update(&self, payment: &mut Payment) -> Result<(), RepoError> {
        stamp_update(payment, self.now());
        let s = payment.snapshot();
        let events = payment.take_events();
        let stamp = self.event_stamp();
        let version = in_transaction!(self, |tx| {
            let version = update_payment(tx, &s).await?;
            insert_events(tx, &events, Some(version), stamp).await?;
            Ok(version)
        })?;
        payment.set_version(version);
        Ok(())
    }
}

/// `PaymentRepo::insert` inside the caller's transaction (also used by `PgBatch`).
pub(crate) async fn insert_payment(
    tx: &impl Queries,
    s: &PaymentSnapshot,
) -> Result<(), RepoError> {
    tx.query_typed(
        &format!(
            "INSERT INTO payments (id, version, order_id, method_id, method_name, provider,
                 currency, amount, refunded, status, reference, instructions, paid_at,
                 voided_at, refund_references, attempt, cart_id)
             VALUES ($1, 1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, {}, {}, $14, $15, $16)",
            ts(12),
            ts(13)
        ),
        &[
            (&s.id.as_uuid(), Type::UUID),
            (&s.order.map(|o| o.as_uuid()), Type::UUID),
            (&s.method.map(|m| m.as_uuid()), Type::UUID),
            (&s.method_name, Type::TEXT),
            (&s.provider.as_str(), Type::TEXT),
            (&s.amount.currency().as_str(), Type::TEXT),
            (&s.amount.amount(), Type::INT8),
            (&s.refunded.amount(), Type::INT8),
            (&s.status.as_str(), Type::TEXT),
            (&s.reference, Type::TEXT),
            (&s.instructions, Type::TEXT),
            (&opt_micros(s.paid_at), Type::INT8),
            (&opt_micros(s.voided_at), Type::INT8),
            (&s.refund_references, Type::TEXT_ARRAY),
            (&s.attempt, Type::TEXT),
            (&s.cart.map(|c| c.as_uuid()), Type::UUID),
        ],
    )
    .await
    .map_err(repo_error)?;
    write_stamps(tx, "payments", "id", &s.id.as_uuid(), Type::UUID, s.stamps).await
}

/// `PaymentRepo::update` inside the caller's transaction (also used by `PgBatch`); the
/// new version.
pub(crate) async fn update_payment(
    tx: &impl Queries,
    s: &PaymentSnapshot,
) -> Result<Version, RepoError> {
    let row = tx
        .query_typed_opt(
            &format!(
                "UPDATE payments SET version = version + 1, order_id = $3, method_id = $4,
                     method_name = $5, provider = $6, currency = $7, amount = $8,
                     refunded = $9, status = $10, reference = $11, instructions = $12,
                     paid_at = {}, voided_at = {}, refund_references = $15,
                     attempt = $16, cart_id = $17
                 WHERE id = $1 AND version = $2 RETURNING version",
                ts(13),
                ts(14)
            ),
            &[
                (&s.id.as_uuid(), Type::UUID),
                (&version_to_db(s.version)?, Type::INT8),
                (&s.order.map(|o| o.as_uuid()), Type::UUID),
                (&s.method.map(|m| m.as_uuid()), Type::UUID),
                (&s.method_name, Type::TEXT),
                (&s.provider.as_str(), Type::TEXT),
                (&s.amount.currency().as_str(), Type::TEXT),
                (&s.amount.amount(), Type::INT8),
                (&s.refunded.amount(), Type::INT8),
                (&s.status.as_str(), Type::TEXT),
                (&s.reference, Type::TEXT),
                (&s.instructions, Type::TEXT),
                (&opt_micros(s.paid_at), Type::INT8),
                (&opt_micros(s.voided_at), Type::INT8),
                (&s.refund_references, Type::TEXT_ARRAY),
                (&s.attempt, Type::TEXT),
                (&s.cart.map(|c| c.as_uuid()), Type::UUID),
            ],
        )
        .await
        .map_err(repo_error)?
        .ok_or(RepoError::StaleVersion)?;
    let version = version_from_db(col(&row, "version")?)?;
    write_stamps(tx, "payments", "id", &s.id.as_uuid(), Type::UUID, s.stamps).await?;
    Ok(version)
}
