use hypha_cart::{Cart, CartLineSnapshot, CartRepo, CartSnapshot, SelectedShipping, StoredQuote};
use hypha_core::{
    CartId, CountryCode, CustomerId, Email, MarketId, Money, OrderId, Price, ProductId, RepoError,
    ShippingOptionId, Timestamp, VariantId, Version, Versioned,
};
use hypha_shipping::ProviderKey;
use tokio_postgres::{
    GenericClient,
    types::{ToSql, Type},
};
use uuid::Uuid;

use crate::{
    columns::{
        address_assignments, address_columns, address_from_row, address_values, currency_from_db,
        micros, ts, version_from_db, version_to_db,
    },
    error::{corrupt, repo_error},
    store::{PgConnection, PgStore, consistent_read},
};

impl<C: PgConnection> CartRepo for PgStore<C> {
    async fn get(&self, id: CartId) -> Result<Option<Cart>, RepoError> {
        let mut conn = self.conn().await;
        consistent_read!(conn.client(), |tx| load_cart(tx, id).await)
    }

    async fn insert(&self, cart: &mut Cart) -> Result<(), RepoError> {
        let s = cart.snapshot();
        let mut conn = self.conn().await;
        let tx = conn.client().transaction().await.map_err(repo_error)?;
        insert_cart(&tx, &s).await?;
        tx.commit().await.map_err(repo_error)?;
        cart.set_version(Version::NEW.next());
        Ok(())
    }

    async fn update(&self, cart: &mut Cart) -> Result<(), RepoError> {
        let s = cart.snapshot();
        let mut conn = self.conn().await;
        let tx = conn.client().transaction().await.map_err(repo_error)?;
        let new_version = update_cart(&tx, &s).await?;
        tx.commit().await.map_err(repo_error)?;
        cart.set_version(new_version);
        Ok(())
    }
}

/// `CartRepo::insert` inside the caller's transaction (also used by `PgBatch`).
pub(crate) async fn insert_cart(
    tx: &impl GenericClient,
    s: &CartSnapshot,
) -> Result<(), RepoError> {
    tx.query_typed(
        "INSERT INTO carts (id, version, market_id, country, email, customer_id, order_id)
         VALUES ($1, 1, $2, $3, $4, $5, $6)",
        &[
            (&s.id.as_uuid(), Type::UUID),
            (&s.market.as_uuid(), Type::UUID),
            (&s.country.as_str(), Type::TEXT),
            (&s.email.as_ref().map(Email::as_str), Type::TEXT),
            (&s.customer.map(|c| c.as_uuid()), Type::UUID),
            (&s.order.map(|o| o.as_uuid()), Type::UUID),
        ],
    )
    .await
    .map_err(repo_error)?;
    write_shipping(tx, s).await?;
    write_lines(tx, s.id, &s.lines).await
}

/// Sets the shipping columns to the snapshot's address and choice (NULL for none).
async fn write_shipping(tx: &impl GenericClient, s: &CartSnapshot) -> Result<(), RepoError> {
    let choice = s.shipping.as_ref();
    let quote = choice.and_then(|c| c.quote.as_ref());
    let id = s.id.as_uuid();
    let address = address_values(s.shipping_address.as_ref());
    let option = choice.map(|c| c.option.as_uuid());
    let amount = quote.map(|q| q.amount.money().amount());
    let currency = quote.map(|q| q.amount.money().currency().as_str());
    let provider = quote.map(|q| q.provider.as_str());
    let service = quote.map(|q| q.service.as_str());
    let reference = quote.and_then(|q| q.reference.as_deref());
    let quoted_at = quote.map(|q| q.quoted_at.unix_micros());
    let mut params: Vec<(&(dyn ToSql + Sync), Type)> = vec![(&id, Type::UUID)];
    params.extend(
        address
            .iter()
            .map(|v| (v as &(dyn ToSql + Sync), Type::TEXT)),
    );
    params.extend([
        (&option as &(dyn ToSql + Sync), Type::UUID),
        (&amount, Type::INT8),
        (&currency, Type::TEXT),
        (&provider, Type::TEXT),
        (&service, Type::TEXT),
        (&reference, Type::TEXT),
        (&quoted_at, Type::INT8),
    ]);
    tx.query_typed(
        &format!(
            "UPDATE carts SET {}, shipping_option_id = $12, quote_amount = $13,
                    quote_currency = $14, quote_provider = $15, quote_service = $16,
                    quote_reference = $17, quoted_at = {}
             WHERE id = $1",
            address_assignments("shipping_", 2),
            ts(18),
        ),
        &params,
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}

/// `CartRepo::update` inside the caller's transaction (also used by `PgBatch`). Returns
/// the new version; on `StaleVersion` the caller drops the transaction, rolling it back.
pub(crate) async fn update_cart(
    tx: &impl GenericClient,
    s: &CartSnapshot,
) -> Result<Version, RepoError> {
    // The version check comes first; the lines are only replaced if it passes.
    let bumped = tx
        .query_typed_opt(
            "UPDATE carts SET version = version + 1, market_id = $3, country = $4, email = $5,
                     customer_id = $6, order_id = $7
                 WHERE id = $1 AND version = $2 RETURNING version",
            &[
                (&s.id.as_uuid(), Type::UUID),
                (&version_to_db(s.version)?, Type::INT8),
                (&s.market.as_uuid(), Type::UUID),
                (&s.country.as_str(), Type::TEXT),
                (&s.email.as_ref().map(Email::as_str), Type::TEXT),
                (&s.customer.map(|c| c.as_uuid()), Type::UUID),
                (&s.order.map(|o| o.as_uuid()), Type::UUID),
            ],
        )
        .await
        .map_err(repo_error)?;
    let Some(row) = bumped else {
        return Err(RepoError::StaleVersion);
    };
    let new_version = version_from_db(row.get(0))?;
    tx.query_typed(
        "DELETE FROM cart_lines WHERE cart_id = $1",
        &[(&s.id.as_uuid(), Type::UUID)],
    )
    .await
    .map_err(repo_error)?;
    write_shipping(tx, s).await?;
    write_lines(tx, s.id, &s.lines).await?;
    Ok(new_version)
}

/// All lines in one statement; `WITH ORDINALITY` keeps their order.
async fn write_lines(
    tx: &impl GenericClient,
    cart: CartId,
    lines: &[CartLineSnapshot],
) -> Result<(), RepoError> {
    let products: Vec<Uuid> = lines.iter().map(|l| l.product.as_uuid()).collect();
    let variants: Vec<Uuid> = lines.iter().map(|l| l.variant.as_uuid()).collect();
    let quantities: Vec<i64> = lines.iter().map(|l| i64::from(l.quantity)).collect();
    tx.query_typed(
        "INSERT INTO cart_lines (cart_id, position, product_id, variant_id, quantity)
         SELECT $1, (n - 1)::integer, p, v, q
         FROM unnest($2::uuid[], $3::uuid[], $4::bigint[]) WITH ORDINALITY AS t (p, v, q, n)",
        &[
            (&cart.as_uuid(), Type::UUID),
            (&products, Type::UUID_ARRAY),
            (&variants, Type::UUID_ARRAY),
            (&quantities, Type::INT8_ARRAY),
        ],
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}

/// A cart and its lines, in 2 queries (run inside `consistent_read`).
async fn load_cart(client: &impl GenericClient, id: CartId) -> Result<Option<Cart>, RepoError> {
    let Some(row) = client
        .query_typed_opt(
            &format!(
                "SELECT version, market_id, country, email, customer_id, order_id, {},
                        shipping_option_id, quote_amount, quote_currency, quote_provider,
                        quote_service, quote_reference, {}
                 FROM carts WHERE id = $1",
                address_columns("shipping_"),
                micros("quoted_at"),
            ),
            &[(&id.as_uuid(), Type::UUID)],
        )
        .await
        .map_err(repo_error)?
    else {
        return Ok(None);
    };
    let lines = client
        .query_typed(
            "SELECT product_id, variant_id, quantity FROM cart_lines
             WHERE cart_id = $1 ORDER BY position",
            &[(&id.as_uuid(), Type::UUID)],
        )
        .await
        .map_err(repo_error)?
        .iter()
        .map(|line| {
            let quantity: i64 = line.get(2);
            Ok(CartLineSnapshot {
                product: ProductId::from_uuid(line.get(0)),
                variant: VariantId::from_uuid(line.get(1)),
                quantity: u32::try_from(quantity)
                    .map_err(|_| corrupt(format!("cart quantity {quantity}")))?,
            })
        })
        .collect::<Result<Vec<_>, RepoError>>()?;
    let country: String = row.get(2);
    let email: Option<String> = row.get(3);
    let snapshot = CartSnapshot {
        id,
        version: version_from_db(row.get(0))?,
        market: MarketId::from_uuid(row.get(1)),
        country: CountryCode::new(&country).map_err(corrupt)?,
        lines,
        email: email
            .as_deref()
            .map(Email::new)
            .transpose()
            .map_err(corrupt)?,
        customer: row.get::<_, Option<Uuid>>(4).map(CustomerId::from_uuid),
        order: row.get::<_, Option<Uuid>>(5).map(OrderId::from_uuid),
        shipping_address: address_from_row(&row, 6),
        shipping: match row.get::<_, Option<Uuid>>(16) {
            None => None,
            Some(option) => Some(SelectedShipping {
                option: ShippingOptionId::from_uuid(option),
                quote: match row.get::<_, Option<i64>>(17) {
                    None => None,
                    Some(amount) => {
                        let currency = currency_from_db(row.get(18))?;
                        Some(StoredQuote {
                            amount: Money::new(amount, currency)
                                .and_then(Price::new)
                                .map_err(corrupt)?,
                            provider: ProviderKey::new(row.get(19)).map_err(corrupt)?,
                            service: row.get(20),
                            reference: row.get(21),
                            quoted_at: Timestamp::from_unix_micros(row.get(22)),
                        })
                    }
                },
            }),
        },
    };
    Cart::restore(snapshot)
        .map(Some)
        .map_err(|e| corrupt(format!("cart {id}: {e}")))
}
