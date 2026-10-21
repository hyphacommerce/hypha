use hypha_cart::{Cart, CartLineSnapshot, CartRepo, CartSnapshot, SelectedShipping, StoredQuote};
use hypha_core::{
    CartId, ChannelId, CountryCode, CustomerId, Email, MarketId, Money, OrderId, PaymentMethodId,
    Price, ProductId, RepoError, ShippingOptionId, Timestamp, VariantId, Version, Versioned,
    stamp_insert, stamp_update,
};
use hypha_shipping::ProviderKey;
use std::collections::HashMap;

use tokio_postgres::{
    Row,
    types::{ToSql, Type},
};
use uuid::Uuid;

use crate::{
    columns::{
        ADDRESS_LEN, address_assignments, address_columns, address_from_row, address_values, col,
        currency_from_db, id_and_address, micros, stamp_columns, stamps_from_row, ts,
        version_from_db, version_to_db, write_stamps,
    },
    error::{corrupt, repo_error},
    store::Queries,
    store::{PgConnection, PgStore, consistent_read, in_transaction},
};

impl<C: PgConnection> CartRepo for PgStore<C> {
    async fn get(&self, id: CartId) -> Result<Option<Cart>, RepoError> {
        let mut conn = self.conn().await;
        consistent_read!(conn, |tx| load_cart(tx, id).await)
    }

    async fn of_customer(&self, customer: CustomerId) -> Result<Vec<Cart>, RepoError> {
        let mut conn = self.conn().await;
        consistent_read!(conn, |tx| load_carts_of(tx, customer).await)
    }

    async fn insert(&self, cart: &mut Cart) -> Result<(), RepoError> {
        stamp_insert(cart, self.now());
        let s = cart.snapshot();
        in_transaction!(self, |tx| insert_cart(tx, &s).await)?;
        cart.set_version(Version::NEW.next());
        Ok(())
    }

    async fn update(&self, cart: &mut Cart) -> Result<(), RepoError> {
        stamp_update(cart, self.now());
        let s = cart.snapshot();
        let new_version = in_transaction!(self, |tx| update_cart(tx, &s).await)?;
        cart.set_version(new_version);
        Ok(())
    }
}

/// `CartRepo::insert` inside the caller's transaction (also used by `PgBatch`).
pub(crate) async fn insert_cart(tx: &impl Queries, s: &CartSnapshot) -> Result<(), RepoError> {
    tx.query_typed(
        "INSERT INTO carts (id, version, market_id, country, email, customer_id, order_id,
             channel_id, payment_method_id, marketing_opt_in)
         VALUES ($1, 1, $2, $3, $4, $5, $6, $7, $8, $9)",
        &[
            (&s.id.as_uuid(), Type::UUID),
            (&s.market.as_uuid(), Type::UUID),
            (&s.country.as_str(), Type::TEXT),
            (&s.email.as_ref().map(Email::as_str), Type::TEXT),
            (&s.customer.map(|c| c.as_uuid()), Type::UUID),
            (&s.order.map(|o| o.as_uuid()), Type::UUID),
            (&s.channel.as_uuid(), Type::UUID),
            (&s.payment_method.map(|m| m.as_uuid()), Type::UUID),
            (&s.marketing_opt_in, Type::BOOL),
        ],
    )
    .await
    .map_err(repo_error)?;
    write_shipping(tx, s).await?;
    write_lines(tx, s.id, &s.lines).await?;
    write_stamps(tx, "carts", "id", &s.id.as_uuid(), Type::UUID, s.stamps).await?;
    Ok(())
}

/// Sets the shipping columns to the snapshot's address and choice (NULL for none).
async fn write_shipping(tx: &impl Queries, s: &CartSnapshot) -> Result<(), RepoError> {
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
    let mut params = id_and_address(&id, &address);
    params.extend([
        (&option as &(dyn ToSql + Sync), Type::UUID),
        (&amount, Type::INT8),
        (&currency, Type::TEXT),
        (&provider, Type::TEXT),
        (&service, Type::TEXT),
        (&reference, Type::TEXT),
        (&quoted_at, Type::INT8),
    ]);
    let n = 2 + ADDRESS_LEN;
    tx.query_typed(
        &format!(
            "UPDATE carts SET {}, shipping_option_id = ${}, quote_amount = ${},
                    quote_currency = ${}, quote_provider = ${}, quote_service = ${},
                    quote_reference = ${}, quoted_at = {}
             WHERE id = $1",
            address_assignments("shipping_", 2),
            n,
            n + 1,
            n + 2,
            n + 3,
            n + 4,
            n + 5,
            ts(n + 6),
        ),
        &params,
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}

/// Removes a cart at `version` (its lines cascade): erasure deleting a customer's open
/// carts (design note 0032, §7.3).
pub(crate) async fn delete_cart(
    tx: &impl Queries,
    id: CartId,
    version: Version,
) -> Result<(), RepoError> {
    let gone = tx
        .query_typed_opt(
            "DELETE FROM carts WHERE id = $1 AND version = $2 RETURNING id",
            &[
                (&id.as_uuid(), Type::UUID),
                (&version_to_db(version)?, Type::INT8),
            ],
        )
        .await
        .map_err(repo_error)?;
    gone.map(|_| ()).ok_or(RepoError::StaleVersion)
}

/// `CartRepo::update` inside the caller's transaction (also used by `PgBatch`). Returns
/// the new version; on `StaleVersion` the caller drops the transaction, rolling it back.
pub(crate) async fn update_cart(tx: &impl Queries, s: &CartSnapshot) -> Result<Version, RepoError> {
    // The version check comes first; the lines are only replaced if it passes.
    let bumped = tx
        .query_typed_opt(
            "UPDATE carts SET version = version + 1, market_id = $3, country = $4, email = $5,
                     customer_id = $6, order_id = $7, payment_method_id = $8,
                     marketing_opt_in = $9
                 WHERE id = $1 AND version = $2 RETURNING version",
            &[
                (&s.id.as_uuid(), Type::UUID),
                (&version_to_db(s.version)?, Type::INT8),
                (&s.market.as_uuid(), Type::UUID),
                (&s.country.as_str(), Type::TEXT),
                (&s.email.as_ref().map(Email::as_str), Type::TEXT),
                (&s.customer.map(|c| c.as_uuid()), Type::UUID),
                (&s.order.map(|o| o.as_uuid()), Type::UUID),
                (&s.payment_method.map(|m| m.as_uuid()), Type::UUID),
                (&s.marketing_opt_in, Type::BOOL),
            ],
        )
        .await
        .map_err(repo_error)?;
    let Some(row) = bumped else {
        return Err(RepoError::StaleVersion);
    };
    let new_version = version_from_db(col(&row, "version")?)?;
    tx.query_typed(
        "DELETE FROM cart_lines WHERE cart_id = $1",
        &[(&s.id.as_uuid(), Type::UUID)],
    )
    .await
    .map_err(repo_error)?;
    write_shipping(tx, s).await?;
    write_lines(tx, s.id, &s.lines).await?;
    let written = new_version;
    write_stamps(tx, "carts", "id", &s.id.as_uuid(), Type::UUID, s.stamps).await?;
    Ok(written)
}

/// All lines in one statement; `WITH ORDINALITY` keeps their order.
async fn write_lines(
    tx: &impl Queries,
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
async fn load_cart(client: &impl Queries, id: CartId) -> Result<Option<Cart>, RepoError> {
    let Some(row) = client
        .query_typed_opt(
            &format!("{} WHERE id = $1", select_carts()),
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
        .map(line_from_row)
        .collect::<Result<Vec<_>, RepoError>>()?;
    cart_from_row(&row, id, lines).map(Some)
}

/// Every cart of `customer`, oldest first, in 2 queries for all of them (run inside
/// `consistent_read`): the rows, then every line of theirs.
async fn load_carts_of(
    client: &impl Queries,
    customer: CustomerId,
) -> Result<Vec<Cart>, RepoError> {
    let rows = client
        .query_typed(
            &format!("{} WHERE customer_id = $1 ORDER BY id", select_carts()),
            &[(&customer.as_uuid(), Type::UUID)],
        )
        .await
        .map_err(repo_error)?;
    let ids: Vec<Uuid> = rows
        .iter()
        .map(|row| col(row, "id"))
        .collect::<Result<_, RepoError>>()?;
    let mut lines: HashMap<Uuid, Vec<CartLineSnapshot>> = HashMap::new();
    for row in client
        .query_typed(
            "SELECT cart_id, product_id, variant_id, quantity FROM cart_lines
             WHERE cart_id = ANY($1) ORDER BY cart_id, position",
            &[(&ids, Type::UUID_ARRAY)],
        )
        .await
        .map_err(repo_error)?
        .iter()
    {
        lines
            .entry(col(row, "cart_id")?)
            .or_default()
            .push(line_from_row(row)?);
    }
    rows.iter()
        .map(|row| {
            let id: Uuid = col(row, "id")?;
            cart_from_row(
                row,
                CartId::from_uuid(id),
                lines.remove(&id).unwrap_or_default(),
            )
        })
        .collect()
}

/// The cart columns a load reads, without the `WHERE`.
fn select_carts() -> String {
    format!(
        "SELECT id, version, market_id, country, email, customer_id, order_id, channel_id,
                payment_method_id, marketing_opt_in, {},
                shipping_option_id, quote_amount, quote_currency, quote_provider,
                quote_service, quote_reference, {}, {}
         FROM carts",
        address_columns("shipping_"),
        micros("quoted_at"),
        stamp_columns(""),
    )
}

fn line_from_row(line: &Row) -> Result<CartLineSnapshot, RepoError> {
    let quantity: i64 = col(line, "quantity")?;
    Ok(CartLineSnapshot {
        product: ProductId::from_uuid(col(line, "product_id")?),
        variant: VariantId::from_uuid(col(line, "variant_id")?),
        quantity: u32::try_from(quantity)
            .map_err(|_| corrupt(format!("cart quantity {quantity}")))?,
    })
}

fn cart_from_row(row: &Row, id: CartId, lines: Vec<CartLineSnapshot>) -> Result<Cart, RepoError> {
    let country: String = col(&row, "country")?;
    let email: Option<String> = col(&row, "email")?;
    let snapshot = CartSnapshot {
        id,
        version: version_from_db(col(&row, "version")?)?,
        stamps: stamps_from_row(&row)?,
        market: MarketId::from_uuid(col(&row, "market_id")?),
        country: CountryCode::new(&country).map_err(corrupt)?,
        channel: ChannelId::from_uuid(col(&row, "channel_id")?),
        lines,
        email: email
            .as_deref()
            .map(Email::new)
            .transpose()
            .map_err(corrupt)?,
        customer: col::<Option<Uuid>>(&row, "customer_id")?.map(CustomerId::from_uuid),
        order: col::<Option<Uuid>>(&row, "order_id")?.map(OrderId::from_uuid),
        payment_method: col::<Option<Uuid>>(&row, "payment_method_id")?
            .map(PaymentMethodId::from_uuid),
        marketing_opt_in: col(&row, "marketing_opt_in")?,
        shipping_address: address_from_row(&row, "shipping_")?,
        shipping: match col::<Option<Uuid>>(&row, "shipping_option_id")? {
            None => None,
            Some(option) => Some(SelectedShipping {
                option: ShippingOptionId::from_uuid(option),
                quote: match col::<Option<i64>>(&row, "quote_amount")? {
                    None => None,
                    Some(amount) => {
                        let currency = currency_from_db(col(&row, "quote_currency")?)?;
                        Some(StoredQuote {
                            amount: Money::new(amount, currency)
                                .and_then(Price::new)
                                .map_err(corrupt)?,
                            provider: ProviderKey::new(col(&row, "quote_provider")?)
                                .map_err(corrupt)?,
                            service: col(&row, "quote_service")?,
                            reference: col(&row, "quote_reference")?,
                            quoted_at: Timestamp::from_unix_micros(col(&row, "quoted_at")?),
                        })
                    }
                },
            }),
        },
    };
    Cart::restore(snapshot).map_err(|e| corrupt(format!("cart {id}: {e}")))
}
