use std::collections::HashMap;

use hypha_core::{
    Address, CartId, CountryCode, CustomerId, Email, MarketId, Money, OrderId, OrderLineId, Page,
    PageRequest, Price, ProductId, Quantity, RepoError, ShippingGroupId, ShippingOptionId,
    TaxClassId, TaxRegionId, Timestamp, VariantId, Version, Versioned,
};
use hypha_orders::{
    Order, OrderLine, OrderNumber, OrderRepo, OrderShippingLine, OrderSnapshot, OrderSummary,
    OrderTax, OrderTaxSource,
};
use tokio_postgres::{GenericClient, Row, types::Type};
use uuid::Uuid;

use crate::{
    columns::{
        ADDRESS_LEN, address_assignments, address_columns, address_from_row, address_values,
        currency_from_db, id_and_address, micros, tax_rate_from_db, tax_rate_to_db, ts,
        version_from_db, version_to_db,
    },
    error::{corrupt, repo_error},
    store::{PgConnection, PgStore, consistent_read},
};

impl<C: PgConnection> OrderRepo for PgStore<C> {
    async fn get(&self, id: OrderId) -> Result<Option<Order>, RepoError> {
        let mut conn = self.conn().await;
        consistent_read!(conn.client(), |tx| {
            Ok(load(tx, &[id.as_uuid()]).await?.pop())
        })
    }

    /// One query whatever the page holds: each order's item count is summed in SQL (per
    /// order, only for the rows the limit keeps), so no line rows leave the database.
    async fn list(&self, page: PageRequest<OrderId>) -> Result<Page<OrderSummary>, RepoError> {
        let after = page.after.map(|id| id.as_uuid());
        let fetch = i64::from(page.limit.get()) + 1;
        let mut conn = self.conn().await;
        let rows = conn
            .client()
            .query_typed(
                &format!(
                    "SELECT o.id, o.number, {},
                     o.customer_id, o.email, o.country, o.currency, o.total,
                     o.financial_status, o.fulfillment_status,
                     (SELECT coalesce(sum(quantity), 0) FROM order_lines
                      WHERE order_id = o.id)::bigint
                 FROM orders o WHERE ($1::uuid IS NULL OR o.id < $1)
                 ORDER BY o.id DESC LIMIT $2",
                    micros("o.placed_at"),
                ),
                &[(&after, Type::UUID), (&fetch, Type::INT8)],
            )
            .await
            .map_err(repo_error)?;
        let summaries = rows
            .iter()
            .map(summary_from_db)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Page::from_overfetch(summaries, page.limit))
    }

    async fn get_by_number(&self, number: OrderNumber) -> Result<Option<Order>, RepoError> {
        let mut conn = self.conn().await;
        consistent_read!(conn.client(), |tx| {
            let found = tx
                .query_typed_opt(
                    "SELECT id FROM orders WHERE number = $1",
                    &[(&number.value(), Type::INT8)],
                )
                .await
                .map_err(repo_error)?;
            match found {
                Some(row) => Ok(load(tx, &[row.get(0)]).await?.pop()),
                None => Ok(None),
            }
        })
    }

    async fn insert(&self, order: &mut Order) -> Result<(), RepoError> {
        let s = order.snapshot();
        let mut conn = self.conn().await;
        let tx = conn.client().transaction().await.map_err(repo_error)?;
        insert_order(&tx, &s).await?;
        tx.commit().await.map_err(repo_error)?;
        order.set_version(Version::NEW.next());
        Ok(())
    }

    /// Writes the whole order like every other repo, not only what today's commands
    /// change (the statuses), so the next field that becomes editable can't be dropped
    /// here while the in-memory repo keeps it. Lines are replaced after the version check.
    async fn update(&self, order: &mut Order) -> Result<(), RepoError> {
        let s = order.snapshot();
        let mut conn = self.conn().await;
        let tx = conn.client().transaction().await.map_err(repo_error)?;
        let bumped = tx
            .query_typed_opt(
                &format!(
                    "UPDATE orders SET version = version + 1, number = $3,
                     placed_at = {},
                     cart_id = $5, market_id = $6, currency = $7, country = $8,
                     taxes_included = $9, customer_id = $10, email = $11, subtotal = $12,
                     tax_total = $13, total = $14, financial_status = $15,
                     fulfillment_status = $16
                 WHERE id = $1 AND version = $2 RETURNING version",
                    ts(4),
                ),
                &[
                    (&s.id.as_uuid(), Type::UUID),
                    (&version_to_db(s.version)?, Type::INT8),
                    (&s.number.value(), Type::INT8),
                    (&s.placed_at.unix_micros(), Type::INT8),
                    (&s.cart.as_uuid(), Type::UUID),
                    (&s.market.as_uuid(), Type::UUID),
                    (&s.currency.as_str(), Type::TEXT),
                    (&s.country.as_str(), Type::TEXT),
                    (&s.taxes_included, Type::BOOL),
                    (&s.customer.as_uuid(), Type::UUID),
                    (&s.email.as_str(), Type::TEXT),
                    (&s.subtotal.amount(), Type::INT8),
                    (&s.tax_total.amount(), Type::INT8),
                    (&s.total.amount(), Type::INT8),
                    (&s.financial_status.as_str(), Type::TEXT),
                    (&s.fulfillment_status.as_str(), Type::TEXT),
                ],
            )
            .await
            .map_err(repo_error)?;
        let Some(row) = bumped else {
            return Err(RepoError::StaleVersion);
        };
        let new_version = version_from_db(row.get(0))?;
        tx.query_typed(
            "DELETE FROM order_lines WHERE order_id = $1",
            &[(&s.id.as_uuid(), Type::UUID)],
        )
        .await
        .map_err(repo_error)?;
        insert_lines(&tx, s.id, &s.lines).await?;
        tx.query_typed(
            "DELETE FROM order_shipping_lines WHERE order_id = $1",
            &[(&s.id.as_uuid(), Type::UUID)],
        )
        .await
        .map_err(repo_error)?;
        write_shipping(&tx, &s).await?;
        tx.commit().await.map_err(repo_error)?;
        order.set_version(new_version);
        Ok(())
    }

    async fn next_number(&self) -> Result<OrderNumber, RepoError> {
        let mut conn = self.conn().await;
        // In a transaction so no query cache (Hyperdrive's) can hand two checkouts the
        // same number; read-write, because `nextval` writes the sequence.
        let tx = conn.client().transaction().await.map_err(repo_error)?;
        let row = tx
            .query_typed_one("SELECT nextval('order_numbers')", &[])
            .await
            .map_err(repo_error)?;
        tx.commit().await.map_err(repo_error)?;
        OrderNumber::new(row.get(0)).map_err(corrupt)
    }
}

/// `OrderRepo::insert` inside the caller's transaction (also used by `PgBatch`): the
/// order row, then every line in one statement.
pub(crate) async fn insert_order(
    tx: &impl GenericClient,
    s: &OrderSnapshot,
) -> Result<(), RepoError> {
    tx.query_typed(
        &format!(
            "INSERT INTO orders (id, version, number, placed_at, cart_id, market_id, currency,
             country, taxes_included, customer_id, email, subtotal, tax_total, total,
             financial_status, fulfillment_status)
         VALUES ($1, 1, $2, {}, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)",
            ts(3),
        ),
        &[
            (&s.id.as_uuid(), Type::UUID),
            (&s.number.value(), Type::INT8),
            (&s.placed_at.unix_micros(), Type::INT8),
            (&s.cart.as_uuid(), Type::UUID),
            (&s.market.as_uuid(), Type::UUID),
            (&s.currency.as_str(), Type::TEXT),
            (&s.country.as_str(), Type::TEXT),
            (&s.taxes_included, Type::BOOL),
            (&s.customer.as_uuid(), Type::UUID),
            (&s.email.as_str(), Type::TEXT),
            (&s.subtotal.amount(), Type::INT8),
            (&s.tax_total.amount(), Type::INT8),
            (&s.total.amount(), Type::INT8),
            (&s.financial_status.as_str(), Type::TEXT),
            (&s.fulfillment_status.as_str(), Type::TEXT),
        ],
    )
    .await
    .map_err(repo_error)?;
    insert_lines(tx, s.id, &s.lines).await?;
    write_shipping(tx, s).await
}

/// The shipping address and total on the order's row, and its shipping lines.
async fn write_shipping(tx: &impl GenericClient, s: &OrderSnapshot) -> Result<(), RepoError> {
    let id = s.id.as_uuid();
    let address = address_values(s.shipping_address.as_ref().map(|a| a.parts()));
    let total = s.shipping_total.amount();
    let mut params = id_and_address(&id, &address);
    params.push((&total, Type::INT8));
    tx.query_typed(
        &format!(
            "UPDATE orders SET {}, shipping_total = ${} WHERE id = $1",
            address_assignments("shipping_", 2),
            2 + ADDRESS_LEN,
        ),
        &params,
    )
    .await
    .map_err(repo_error)?;
    // A handful of lines at most (one per group): one statement each.
    for (position, line) in (0i32..).zip(&s.shipping_lines) {
        let line_ids: Vec<Uuid> = line.lines.iter().map(|l| l.as_uuid()).collect();
        let tax = line.tax.as_ref();
        tx.query_typed(
            "INSERT INTO order_shipping_lines (order_id, position, group_id, line_ids,
                 option_id, code, name, provider, service, reference, amount,
                 estimate_min_days, estimate_max_days, tax_rate_ppm, tax_class_id,
                 tax_amount, tax_region_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)",
            &[
                (&s.id.as_uuid(), Type::UUID),
                (&position, Type::INT4),
                (&line.group.as_uuid(), Type::UUID),
                (&line_ids, Type::UUID_ARRAY),
                (&line.option.as_uuid(), Type::UUID),
                (&line.code, Type::TEXT),
                (&line.name, Type::TEXT),
                (&line.provider, Type::TEXT),
                (&line.service, Type::TEXT),
                (&line.reference, Type::TEXT),
                (&line.amount.money().amount(), Type::INT8),
                (
                    &line.estimate_days.map(|(min, _)| i32::from(min)),
                    Type::INT4,
                ),
                (
                    &line.estimate_days.map(|(_, max)| i32::from(max)),
                    Type::INT4,
                ),
                (
                    &tax.map(|t| tax_rate_to_db(t.rate)).transpose()?,
                    Type::INT4,
                ),
                (&tax.and_then(|t| t.class).map(|c| c.as_uuid()), Type::UUID),
                (&tax.map(|t| t.amount.amount()), Type::INT8),
                (
                    &tax.map(|t| match t.source {
                        OrderTaxSource::RateTable { region } => region.as_uuid(),
                    }),
                    Type::UUID,
                ),
            ],
        )
        .await
        .map_err(repo_error)?;
    }
    Ok(())
}

/// All lines in one statement: each column as an array, `unnest` turns them back into
/// rows, `WITH ORDINALITY` numbers them, which is the position.
async fn insert_lines(
    tx: &impl GenericClient,
    order: OrderId,
    lines: &[OrderLine],
) -> Result<(), RepoError> {
    let ids: Vec<Uuid> = lines.iter().map(|l| l.id.as_uuid()).collect();
    let products: Vec<Uuid> = lines.iter().map(|l| l.product.as_uuid()).collect();
    let variants: Vec<Uuid> = lines.iter().map(|l| l.variant.as_uuid()).collect();
    let quantities: Vec<i64> = lines
        .iter()
        .map(|l| i64::from(l.quantity.value()))
        .collect();
    let titles: Vec<&str> = lines.iter().map(|l| l.product_title.as_str()).collect();
    let variant_titles: Vec<Option<&str>> =
        lines.iter().map(|l| l.variant_title.as_deref()).collect();
    let skus: Vec<Option<&str>> = lines.iter().map(|l| l.sku.as_deref()).collect();
    let unit_prices: Vec<i64> = lines
        .iter()
        .map(|l| l.unit_price.money().amount())
        .collect();
    let totals: Vec<i64> = lines.iter().map(|l| l.total.amount()).collect();
    let tax_rates = lines
        .iter()
        .map(|l| l.tax.map(|t| tax_rate_to_db(t.rate)).transpose())
        .collect::<Result<Vec<Option<i32>>, _>>()?;
    let tax_classes: Vec<Option<Uuid>> = lines
        .iter()
        .map(|l| l.tax.and_then(|t| t.class).map(|c| c.as_uuid()))
        .collect();
    let tax_amounts: Vec<Option<i64>> = lines
        .iter()
        .map(|l| l.tax.map(|t| t.amount.amount()))
        .collect();
    let tax_regions: Vec<Option<Uuid>> = lines
        .iter()
        .map(|l| {
            l.tax.map(|t| match t.source {
                OrderTaxSource::RateTable { region } => region.as_uuid(),
            })
        })
        .collect();
    tx.query_typed(
        "INSERT INTO order_lines (order_id, position, id, product_id, variant_id, quantity,
             product_title, variant_title, sku, unit_price, total, tax_rate_ppm, tax_class_id,
             tax_amount, tax_region_id)
         SELECT $1, (n - 1)::integer, id, product_id, variant_id, quantity, product_title,
             variant_title, sku, unit_price, total, tax_rate_ppm, tax_class_id, tax_amount,
             tax_region_id
         FROM unnest($2::uuid[], $3::uuid[], $4::uuid[], $5::bigint[], $6::text[], $7::text[],
                     $8::text[], $9::bigint[], $10::bigint[], $11::integer[], $12::uuid[],
                     $13::bigint[], $14::uuid[])
              WITH ORDINALITY AS t (id, product_id, variant_id, quantity, product_title,
                  variant_title, sku, unit_price, total, tax_rate_ppm, tax_class_id,
                  tax_amount, tax_region_id, n)",
        &[
            (&order.as_uuid(), Type::UUID),
            (&ids, Type::UUID_ARRAY),
            (&products, Type::UUID_ARRAY),
            (&variants, Type::UUID_ARRAY),
            (&quantities, Type::INT8_ARRAY),
            (&titles, Type::TEXT_ARRAY),
            (&variant_titles, Type::TEXT_ARRAY),
            (&skus, Type::TEXT_ARRAY),
            (&unit_prices, Type::INT8_ARRAY),
            (&totals, Type::INT8_ARRAY),
            (&tax_rates, Type::INT4_ARRAY),
            (&tax_classes, Type::UUID_ARRAY),
            (&tax_amounts, Type::INT8_ARRAY),
            (&tax_regions, Type::UUID_ARRAY),
        ],
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}

/// The orders with these IDs (missing ones are skipped) and their lines, in 2 queries
/// (run inside `consistent_read`).
/// Where `load`'s order rows have the shipping address (`shipping_total` follows it).
const ADDRESS_AT: usize = 16;

async fn load(client: &impl GenericClient, ids: &[Uuid]) -> Result<Vec<Order>, RepoError> {
    if ids.is_empty() {
        return Ok(vec![]);
    }
    let ids = ids.to_vec();
    let param = [(
        &ids as &(dyn tokio_postgres::types::ToSql + Sync),
        Type::UUID_ARRAY,
    )];

    let orders = client
        .query_typed(
            &format!(
                "SELECT id, version, number, {}, cart_id, market_id, currency, country,
                     taxes_included, customer_id, email, subtotal, tax_total, total,
                     financial_status, fulfillment_status, {}, shipping_total
                 FROM orders WHERE id = ANY($1)",
                micros("placed_at"),
                address_columns("shipping_"),
            ),
            &param,
        )
        .await
        .map_err(repo_error)?;
    if orders.is_empty() {
        return Ok(vec![]);
    }

    let mut lines: HashMap<Uuid, Vec<Row>> = HashMap::new();
    for line in client
        .query_typed(
            "SELECT id, product_id, variant_id, quantity, product_title, variant_title, sku,
                 unit_price, total, tax_rate_ppm, tax_class_id, tax_amount, tax_region_id,
                 order_id
             FROM order_lines WHERE order_id = ANY($1) ORDER BY order_id, position",
            &param,
        )
        .await
        .map_err(repo_error)?
    {
        lines.entry(line.get(13)).or_default().push(line);
    }

    let mut shipping: HashMap<Uuid, Vec<Row>> = HashMap::new();
    for line in client
        .query_typed(
            "SELECT group_id, line_ids, option_id, code, name, provider, service, reference,
                 amount, estimate_min_days, estimate_max_days, tax_rate_ppm, tax_class_id,
                 tax_amount, tax_region_id, order_id
             FROM order_shipping_lines WHERE order_id = ANY($1) ORDER BY order_id, position",
            &param,
        )
        .await
        .map_err(repo_error)?
    {
        shipping.entry(line.get(15)).or_default().push(line);
    }

    orders
        .iter()
        .map(|row| {
            let id: Uuid = row.get(0);
            order_from_db(
                row,
                lines.remove(&id).unwrap_or_default(),
                shipping.remove(&id).unwrap_or_default(),
            )
        })
        .collect()
}

/// An order from its row (`load`'s columns) and its line rows, in position order.
fn order_from_db(
    row: &Row,
    line_rows: Vec<Row>,
    shipping_rows: Vec<Row>,
) -> Result<Order, RepoError> {
    let currency_code: String = row.get(6);
    let currency = currency_from_db(&currency_code)?;
    let money = |amount: i64| Money::new(amount, currency).map_err(corrupt);
    let lines = line_rows
        .iter()
        .map(|line| line_from_db(line, &money))
        .collect::<Result<Vec<_>, RepoError>>()?;
    let shipping_lines = shipping_rows
        .iter()
        .map(|line| shipping_line_from_db(line, &money))
        .collect::<Result<Vec<_>, RepoError>>()?;
    let shipping_address = address_from_row(row, ADDRESS_AT)
        .map(Address::new)
        .transpose()
        .map_err(corrupt)?;

    let country: String = row.get(7);
    let email: String = row.get(10);
    let financial: String = row.get(14);
    let fulfillment: String = row.get(15);
    let snapshot = OrderSnapshot {
        id: OrderId::from_uuid(row.get(0)),
        version: version_from_db(row.get(1))?,
        number: OrderNumber::new(row.get(2)).map_err(corrupt)?,
        placed_at: Timestamp::from_unix_micros(row.get(3)),
        cart: CartId::from_uuid(row.get(4)),
        market: MarketId::from_uuid(row.get(5)),
        currency,
        country: CountryCode::new(&country).map_err(corrupt)?,
        taxes_included: row.get(8),
        customer: CustomerId::from_uuid(row.get(9)),
        email: Email::new(&email).map_err(corrupt)?,
        lines,
        shipping_address,
        shipping_lines,
        subtotal: money(row.get(11))?,
        shipping_total: money(row.get(ADDRESS_AT + ADDRESS_LEN))?,
        tax_total: money(row.get(12))?,
        total: money(row.get(13))?,
        financial_status: financial.parse().map_err(corrupt)?,
        fulfillment_status: fulfillment.parse().map_err(corrupt)?,
    };
    Order::restore(snapshot).map_err(corrupt)
}

/// An order summary from `list`'s row.
fn summary_from_db(row: &Row) -> Result<OrderSummary, RepoError> {
    let id: Uuid = row.get(0);
    let email: String = row.get(4);
    let country: String = row.get(5);
    let currency: String = row.get(6);
    let financial: String = row.get(8);
    let fulfillment: String = row.get(9);
    let items: i64 = row.get(10);
    Ok(OrderSummary {
        id: OrderId::from_uuid(id),
        number: OrderNumber::new(row.get(1)).map_err(corrupt)?,
        placed_at: Timestamp::from_unix_micros(row.get(2)),
        customer: CustomerId::from_uuid(row.get(3)),
        email: Email::new(&email).map_err(corrupt)?,
        country: CountryCode::new(&country).map_err(corrupt)?,
        financial_status: financial.parse().map_err(corrupt)?,
        fulfillment_status: fulfillment.parse().map_err(corrupt)?,
        total: Money::new(row.get(7), currency_from_db(&currency)?).map_err(corrupt)?,
        item_count: u64::try_from(items).map_err(|_| corrupt(format!("order {id} item count")))?,
    })
}

fn shipping_line_from_db(
    line: &Row,
    money: &impl Fn(i64) -> Result<Money, RepoError>,
) -> Result<OrderShippingLine, RepoError> {
    let rate: Option<i32> = line.get(11);
    let class: Option<Uuid> = line.get(12);
    let amount: Option<i64> = line.get(13);
    let region: Option<Uuid> = line.get(14);
    let tax = match (rate, amount, region) {
        (Some(rate), Some(amount), Some(region)) => Some(OrderTax {
            rate: tax_rate_from_db(rate)?,
            class: class.map(TaxClassId::from_uuid),
            amount: money(amount)?,
            source: OrderTaxSource::RateTable {
                region: TaxRegionId::from_uuid(region),
            },
        }),
        (None, None, None) => None,
        _ => return Err(corrupt("order shipping tax columns are partly set")),
    };
    let days = |i: usize| -> Result<Option<u16>, RepoError> {
        line.get::<_, Option<i32>>(i)
            .map(|d| u16::try_from(d).map_err(corrupt))
            .transpose()
    };
    let line_ids: Vec<Uuid> = line.get(1);
    Ok(OrderShippingLine {
        group: ShippingGroupId::from_uuid(line.get(0)),
        lines: line_ids.into_iter().map(OrderLineId::from_uuid).collect(),
        option: ShippingOptionId::from_uuid(line.get(2)),
        code: line.get(3),
        name: line.get(4),
        provider: line.get(5),
        service: line.get(6),
        reference: line.get(7),
        amount: Price::new(money(line.get(8))?).map_err(corrupt)?,
        estimate_days: days(9)?.zip(days(10)?),
        tax,
    })
}

fn line_from_db(
    line: &Row,
    money: &impl Fn(i64) -> Result<Money, RepoError>,
) -> Result<OrderLine, RepoError> {
    let quantity: i64 = line.get(3);
    let quantity = u32::try_from(quantity)
        .ok()
        .and_then(|q| Quantity::new(q).ok())
        .ok_or_else(|| corrupt(format!("order line quantity {quantity}")))?;
    let rate: Option<i32> = line.get(9);
    let class: Option<Uuid> = line.get(10);
    let amount: Option<i64> = line.get(11);
    let region: Option<Uuid> = line.get(12);
    let tax = match (rate, amount, region) {
        (Some(rate), Some(amount), Some(region)) => Some(OrderTax {
            rate: tax_rate_from_db(rate)?,
            class: class.map(TaxClassId::from_uuid),
            amount: money(amount)?,
            source: OrderTaxSource::RateTable {
                region: TaxRegionId::from_uuid(region),
            },
        }),
        (None, None, None) => None,
        _ => return Err(corrupt("order line tax columns are partly set")),
    };
    Ok(OrderLine {
        id: OrderLineId::from_uuid(line.get(0)),
        product: ProductId::from_uuid(line.get(1)),
        variant: VariantId::from_uuid(line.get(2)),
        quantity,
        product_title: line.get(4),
        variant_title: line.get(5),
        sku: line.get(6),
        unit_price: Price::new(money(line.get(7))?).map_err(corrupt)?,
        total: money(line.get(8))?,
        tax,
    })
}
