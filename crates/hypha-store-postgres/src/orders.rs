use std::collections::HashMap;

use hypha_core::{
    CartId, CountryCode, CustomerId, Email, MarketId, Money, OrderId, OrderLineId, Page,
    PageRequest, Price, ProductId, Quantity, RepoError, TaxClassId, TaxRegionId, Timestamp,
    VariantId, Version, Versioned,
};
use hypha_orders::{
    Order, OrderLine, OrderNumber, OrderRepo, OrderSnapshot, OrderSummary, OrderTax, OrderTaxSource,
};
use tokio_postgres::{GenericClient, Row, types::Type};
use uuid::Uuid;

use crate::{
    columns::{currency_from_db, tax_rate_from_db, tax_rate_to_db, version_from_db, version_to_db},
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
                "SELECT o.id, o.number, (extract(epoch FROM o.placed_at) * 1000000)::bigint,
                     o.customer_id, o.email, o.country, o.currency, o.total,
                     o.financial_status, o.fulfillment_status,
                     (SELECT coalesce(sum(quantity), 0) FROM order_lines
                      WHERE order_id = o.id)::bigint
                 FROM orders o WHERE ($1::uuid IS NULL OR o.id < $1)
                 ORDER BY o.id DESC LIMIT $2",
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
                "UPDATE orders SET version = version + 1, number = $3,
                     placed_at = 'epoch'::timestamptz + $4 * interval '1 microsecond',
                     cart_id = $5, market_id = $6, currency = $7, country = $8,
                     taxes_included = $9, customer_id = $10, email = $11, subtotal = $12,
                     tax_total = $13, total = $14, financial_status = $15,
                     fulfillment_status = $16
                 WHERE id = $1 AND version = $2 RETURNING version",
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
        "INSERT INTO orders (id, version, number, placed_at, cart_id, market_id, currency,
             country, taxes_included, customer_id, email, subtotal, tax_total, total,
             financial_status, fulfillment_status)
         VALUES ($1, 1, $2, 'epoch'::timestamptz + $3 * interval '1 microsecond', $4, $5, $6,
             $7, $8, $9, $10, $11, $12, $13, $14, $15)",
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
    insert_lines(tx, s.id, &s.lines).await
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
            "SELECT id, version, number, (extract(epoch FROM placed_at) * 1000000)::bigint,
                 cart_id, market_id, currency, country, taxes_included, customer_id, email,
                 subtotal, tax_total, total, financial_status, fulfillment_status
             FROM orders WHERE id = ANY($1)",
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

    orders
        .iter()
        .map(|row| {
            let id: Uuid = row.get(0);
            order_from_db(row, lines.remove(&id).unwrap_or_default())
        })
        .collect()
}

/// An order from its row (`load`'s columns) and its line rows, in position order.
fn order_from_db(row: &Row, line_rows: Vec<Row>) -> Result<Order, RepoError> {
    let currency_code: String = row.get(6);
    let currency = currency_from_db(&currency_code)?;
    let money = |amount: i64| Money::new(amount, currency).map_err(corrupt);
    let lines = line_rows
        .iter()
        .map(|line| line_from_db(line, &money))
        .collect::<Result<Vec<_>, RepoError>>()?;

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
        subtotal: money(row.get(11))?,
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
