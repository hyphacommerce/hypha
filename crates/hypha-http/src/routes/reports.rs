//! Reports: what the store's orders add up to (`hypha-order-actions`' `report`).

use axum::{Json, extract::State};
use hypha_core::{ChannelId, Currency, MarketId, Money, ProductId, Timestamp, Timezone};
use hypha_order_actions::{
    Interval, SalesBucket, SalesReport, SalesReportInput, SalesRow, sales_report,
};
use hypha_stores::StoreRepo;
use serde::{Deserialize, Serialize};

use super::rfc3339;
use crate::{
    error::ApiError,
    extract::{Caller, Query},
    state::{SharedState, Storage},
};

/// How long each bucket is, in the store's time zone. Weeks start on Monday.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum IntervalDto {
    Day,
    Week,
    Month,
}

impl From<IntervalDto> for Interval {
    fn from(i: IntervalDto) -> Self {
        match i {
            IntervalDto::Day => Interval::Day,
            IntervalDto::Week => Interval::Week,
            IntervalDto::Month => Interval::Month,
        }
    }
}

impl From<Interval> for IntervalDto {
    fn from(i: Interval) -> Self {
        match i {
            Interval::Day => IntervalDto::Day,
            Interval::Week => IntervalDto::Week,
            Interval::Month => IntervalDto::Month,
        }
    }
}

#[derive(Deserialize, utoipa::IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct SalesReportQuery {
    /// The period's start (RFC 3339); 7 days before `to` when absent.
    from: Option<String>,
    /// The period's end (RFC 3339), left out; the start of tomorrow in the store's time
    /// zone when absent, so the default period is the last 7 days, today included.
    to: Option<String>,
    /// `day` (the default), `week` or `month`. At most 366 buckets
    /// (`422 period_too_long`).
    #[param(inline)]
    interval: Option<IntervalDto>,
    /// Only orders placed through this sales channel.
    #[param(value_type = Option<ChannelId>)]
    channel: Option<ChannelId>,
    /// Only orders placed in this market.
    #[param(value_type = Option<MarketId>)]
    market: Option<MarketId>,
    /// How many products each top list holds: 1 to 20, default 5 (`422 invalid_top`).
    top: Option<u32>,
}

/// One currency's sales. Amounts never mix currencies: a period sold in two has a row
/// for each.
#[derive(Serialize, utoipa::ToSchema)]
pub struct SalesRowResponse {
    currency: Currency,
    /// Orders placed, cancelled ones left out.
    orders_count: u64,
    /// The line totals before shipping (tax included where the market's prices include
    /// it). Orders have no discounts yet, so there's no discount column.
    gross_sales: Money,
    shipping: Money,
    taxes: Money,
    /// What the orders came to.
    total: Money,
    /// What the orders' payments gave back so far, counted with the orders (when they
    /// were placed), not when the money went back.
    refunded: Money,
    /// `total` less `refunded`.
    net: Money,
}

impl From<&SalesRow> for SalesRowResponse {
    fn from(r: &SalesRow) -> Self {
        Self {
            currency: r.currency,
            orders_count: r.orders_count,
            gross_sales: r.gross_sales,
            shipping: r.shipping,
            taxes: r.taxes,
            total: r.total,
            refunded: r.refunded,
            net: r.net,
        }
    }
}

/// A stretch of the period.
#[derive(Serialize, utoipa::ToSchema)]
pub struct SalesBucketResponse {
    /// RFC 3339 in UTC; a bucket starts at a start of day, week or month in the store's
    /// time zone, except the first, which starts at `from`.
    start: String,
    /// Where the next bucket starts; the last one's is `to`.
    end: String,
    /// Per currency, by currency code; empty when nothing sold.
    sales: Vec<SalesRowResponse>,
    /// Orders placed in it and cancelled since: in no sum.
    cancelled_count: u64,
}

impl SalesBucketResponse {
    fn new(b: &SalesBucket) -> Result<Self, ApiError> {
        Ok(Self {
            start: rfc3339(b.start)?,
            end: rfc3339(b.end)?,
            sales: b.sales.iter().map(SalesRowResponse::from).collect(),
            cancelled_count: b.cancelled_count,
        })
    }
}

/// A product in a top list, named as its line in the newest order counted.
#[derive(Serialize, utoipa::ToSchema)]
pub struct ProductUnitsResponse {
    product: ProductId,
    title: String,
    units: u64,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct ProductRevenueResponse {
    product: ProductId,
    title: String,
    units: u64,
    /// Its line totals.
    revenue: Money,
}

/// The products that brought the most in one currency.
#[derive(Serialize, utoipa::ToSchema)]
pub struct CurrencyTopResponse {
    currency: Currency,
    products: Vec<ProductRevenueResponse>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct TopProductsResponse {
    /// Most units first, then by product ID; units count in every currency.
    by_units: Vec<ProductUnitsResponse>,
    /// Per currency, by currency code: most revenue first, then by product ID.
    by_revenue: Vec<CurrencyTopResponse>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct SalesReportResponse {
    /// RFC 3339 in UTC.
    from: String,
    to: String,
    interval: IntervalDto,
    /// The store's time zone the buckets follow; `null`: UTC.
    time_zone: Option<Timezone>,
    /// Every bucket in order, empty ones too.
    buckets: Vec<SalesBucketResponse>,
    /// The whole period.
    total: SalesBucketResponse,
    top_products: TopProductsResponse,
}

impl SalesReportResponse {
    fn new(r: &SalesReport) -> Result<Self, ApiError> {
        Ok(Self {
            from: rfc3339(r.from)?,
            to: rfc3339(r.to)?,
            interval: r.interval.into(),
            time_zone: r.zone,
            buckets: r
                .buckets
                .iter()
                .map(SalesBucketResponse::new)
                .collect::<Result<_, _>>()?,
            total: SalesBucketResponse::new(&r.total)?,
            top_products: TopProductsResponse {
                by_units: r
                    .top_by_units
                    .iter()
                    .map(|p| ProductUnitsResponse {
                        product: p.product,
                        title: p.title.clone(),
                        units: p.units,
                    })
                    .collect(),
                by_revenue: r
                    .top_by_revenue
                    .iter()
                    .map(|c| CurrencyTopResponse {
                        currency: c.currency,
                        products: c
                            .products
                            .iter()
                            .map(|p| ProductRevenueResponse {
                                product: p.product,
                                title: p.title.clone(),
                                units: p.units,
                                revenue: p.revenue,
                            })
                            .collect(),
                    })
                    .collect(),
            },
        })
    }
}

/// An RFC 3339 query parameter.
fn instant(name: &str, value: Option<String>) -> Result<Option<Timestamp>, ApiError> {
    value
        .map(|v| {
            v.parse::<jiff::Timestamp>()
                .map(|t| Timestamp::from_unix_micros(t.as_microsecond()))
                .map_err(|e| {
                    ApiError::invalid_query(format!("`{name}` is an RFC 3339 instant: {e}"))
                })
        })
        .transpose()
}

/// What the period's orders add up to, per day, week or month in the store's time zone
/// and per currency, with what their payments gave back and the products that sold most.
/// Cancelled orders are counted apart and add nothing.
#[utoipa::path(
    get,
    path = "/admin/reports/sales",
    params(SalesReportQuery),
    responses((status = 200, description = "OK", body = SalesReportResponse)),
    extensions(("x-hypha-scopes" = json!(["orders:read"]))),
)]
pub async fn sales<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    query: Query<SalesReportQuery>,
) -> Result<Json<SalesReportResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let query = query.parse()?;
    // Only the zone: the store's details are `stores:read`, the zone is how a report reads.
    let zone = StoreRepo::get(&ports, state.store)
        .await
        .map_err(ApiError::internal)?
        .and_then(|store| store.timezone());
    let input = SalesReportInput {
        from: instant("from", query.from)?,
        to: instant("to", query.to)?,
        interval: query.interval.map(Into::into).unwrap_or_default(),
        channel: query.channel,
        market: query.market,
        top: query.top,
        zone,
    };
    let report = sales_report(&ctx, &ports, &state.clock, input).await?;
    Ok(Json(SalesReportResponse::new(&report)?))
}
