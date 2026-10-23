//! `OrderRepo::sales`: the sales report's figures summed in SQL over `orders_by_placed_at`
//! (migration 0069), four aggregate queries sent together in one snapshot (refunds from
//! the orders' payments).

use futures_util::future::try_join4;
use hypha_core::{Currency, Money, ProductId, RepoError};
use hypha_orders::{
    BucketFigures, CurrencySales, CurrencyTop, ProductRevenue, ProductUnits, SalesFigures,
    SalesQuery, sales::capped,
};
use tokio_postgres::{
    Row,
    types::{ToSql, Type},
};

use crate::{
    columns::{col, currency_from_db},
    error::{corrupt, repo_error},
    store::Queries,
};

/// The orders a query counts, cancelled or not: `o` in the period, channel and market.
/// `$1` the starts (µs), `$2` the end (µs), `$3` the channel, `$4` the market.
const COUNTED: &str = "o.placed_at >= 'epoch'::timestamptz + $1[1] * interval '1 microsecond'
     AND o.placed_at < 'epoch'::timestamptz + $2 * interval '1 microsecond'
     AND ($3::uuid IS NULL OR o.channel_id = $3)
     AND ($4::uuid IS NULL OR o.market_id = $4)";

/// Which start an order falls after, from 1: `width_bucket` over the starts, compared in
/// microseconds as `Timestamp` holds them.
const BUCKET: &str =
    "width_bucket((extract(epoch FROM o.placed_at) * 1000000)::bigint, $1::bigint[])";

pub(crate) async fn sales(
    tx: &impl Queries,
    query: &SalesQuery,
) -> Result<SalesFigures, RepoError> {
    if !query.is_valid() {
        return Err(RepoError::Backend(format!(
            "an invalid sales query: {query:?}"
        )));
    }
    let starts: Vec<i64> = query.starts.iter().map(|t| t.unix_micros()).collect();
    let end = query.end.unix_micros();
    let channel = query.channel.map(|c| c.as_uuid());
    let market = query.market.map(|m| m.as_uuid());
    let top = i64::from(query.top);
    let params: [(&(dyn ToSql + Sync), Type); 5] = [
        (&starts, Type::INT8_ARRAY),
        (&end, Type::INT8),
        (&channel, Type::UUID),
        (&market, Type::UUID),
        (&top, Type::INT8),
    ];
    let totals_sql = format!(
        "SELECT {BUCKET} AS bucket, o.currency,
                count(*) FILTER (WHERE o.cancelled_at IS NULL)::bigint AS orders_count,
                count(*) FILTER (WHERE o.cancelled_at IS NOT NULL)::bigint AS cancelled_count,
                coalesce(sum(o.subtotal) FILTER (WHERE o.cancelled_at IS NULL), 0)::text
                    AS subtotal,
                coalesce(sum(o.shipping_total) FILTER (WHERE o.cancelled_at IS NULL), 0)::text
                    AS shipping,
                coalesce(sum(o.tax_total) FILTER (WHERE o.cancelled_at IS NULL), 0)::text
                    AS taxes,
                coalesce(sum(o.total) FILTER (WHERE o.cancelled_at IS NULL), 0)::text AS total
         FROM orders o
         WHERE {COUNTED}
         GROUP BY 1, o.currency ORDER BY 1, o.currency"
    );
    // A product's title is its line's in the newest order counted.
    let units_sql = format!(
        "SELECT l.product_id, sum(l.quantity)::bigint AS units,
                (array_agg(l.product_title ORDER BY o.placed_at DESC, o.id DESC, l.position))[1]
                    AS title
         FROM order_lines l JOIN orders o ON o.id = l.order_id
         WHERE {COUNTED} AND o.cancelled_at IS NULL
         GROUP BY l.product_id ORDER BY units DESC, l.product_id LIMIT $5"
    );
    let revenue_sql = format!(
        "SELECT currency, product_id, units, revenue, title FROM (
             SELECT o.currency, l.product_id, sum(l.quantity)::bigint AS units,
                    sum(l.total)::text AS revenue,
                    (array_agg(l.product_title
                         ORDER BY o.placed_at DESC, o.id DESC, l.position))[1] AS title,
                    row_number() OVER (PARTITION BY o.currency
                                       ORDER BY sum(l.total) DESC, l.product_id) AS rank
             FROM order_lines l JOIN orders o ON o.id = l.order_id
             WHERE {COUNTED} AND o.cancelled_at IS NULL
             GROUP BY o.currency, l.product_id
         ) AS ranked
         WHERE rank <= $5 ORDER BY currency, rank"
    );
    // `payments_by_order` serves the join; only orders that gave money back have any.
    let refunded_sql = format!(
        "SELECT {BUCKET} AS bucket, o.currency, sum(p.refunded)::text AS refunded
         FROM orders o JOIN payments p ON p.order_id = o.id
         WHERE {COUNTED} AND o.cancelled_at IS NULL
           AND o.financial_status IN ('partially_refunded', 'refunded')
         GROUP BY 1, o.currency"
    );
    let (totals, units, revenue, refunded) = try_join4(
        tx.query_typed(&totals_sql, &params),
        tx.query_typed(&units_sql, &params),
        tx.query_typed(&revenue_sql, &params),
        tx.query_typed(&refunded_sql, &params),
    )
    .await
    .map_err(repo_error)?;

    let mut buckets: Vec<BucketFigures> = vec![BucketFigures::default(); query.starts.len()];
    for row in &totals {
        let bucket = bucket_of(row, buckets.len())?;
        let currency = currency_from_db(&col::<String>(row, "currency")?)?;
        let figures = &mut buckets[bucket];
        figures.cancelled_count += count(row, "cancelled_count")?;
        let orders_count = count(row, "orders_count")?;
        if orders_count == 0 {
            continue;
        }
        figures.sales.push(CurrencySales {
            currency,
            orders_count,
            subtotal: sum(row, "subtotal", currency)?,
            shipping: sum(row, "shipping", currency)?,
            taxes: sum(row, "taxes", currency)?,
            total: sum(row, "total", currency)?,
            refunded: Money::zero(currency),
        });
    }

    let top_by_units = units
        .iter()
        .map(|row| {
            Ok(ProductUnits {
                product: ProductId::from_uuid(col(row, "product_id")?),
                title: col(row, "title")?,
                units: count(row, "units")?,
            })
        })
        .collect::<Result<_, RepoError>>()?;

    let mut top_by_revenue: Vec<CurrencyTop> = Vec::new();
    for row in &revenue {
        let currency = currency_from_db(&col::<String>(row, "currency")?)?;
        let entry = ProductRevenue {
            product: ProductId::from_uuid(col(row, "product_id")?),
            title: col(row, "title")?,
            units: count(row, "units")?,
            revenue: sum(row, "revenue", currency)?,
        };
        match top_by_revenue.last_mut() {
            Some(top) if top.currency == currency => top.products.push(entry),
            _ => top_by_revenue.push(CurrencyTop {
                currency,
                products: vec![entry],
            }),
        }
    }

    for row in &refunded {
        let bucket = bucket_of(row, buckets.len())?;
        let currency = currency_from_db(&col::<String>(row, "currency")?)?;
        // Orders that gave money back were counted, so their currency has its row.
        if let Some(sales) = buckets[bucket]
            .sales
            .iter_mut()
            .find(|s| s.currency == currency)
        {
            sales.refunded = sum(row, "refunded", currency)?;
        }
    }

    Ok(SalesFigures {
        buckets,
        top_by_units,
        top_by_revenue,
    })
}

/// `width_bucket`'s answer as an index: 1 is the first start's bucket.
fn bucket_of(row: &Row, buckets: usize) -> Result<usize, RepoError> {
    let n: i32 = col(row, "bucket")?;
    usize::try_from(n)
        .ok()
        .and_then(|n| n.checked_sub(1))
        .filter(|n| *n < buckets)
        .ok_or_else(|| corrupt(format!("sales bucket {n} of {buckets}")))
}

fn count(row: &Row, name: &str) -> Result<u64, RepoError> {
    let n: i64 = col(row, name)?;
    u64::try_from(n).map_err(|e| corrupt(format!("{name} {n}: {e}")))
}

/// A `sum` of amounts (numeric, read as text), capped like `SalesFigures::of`.
fn sum(row: &Row, name: &str, currency: Currency) -> Result<Money, RepoError> {
    let text: String = col(row, name)?;
    let amount = text
        .parse::<i128>()
        .map_err(|e| corrupt(format!("{name} {text:?}: {e}")))?;
    Ok(capped(amount, currency))
}
