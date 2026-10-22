//! The sales report: a period's orders added up per bucket of time in the store's time
//! zone, per currency, with what their payments gave back and the products that sold most.
//!
//! The sums come from storage (`OrderRepo::sales`); this workflow decides the buckets
//! (calendar days, weeks and months need the zone's rules, which storage doesn't share)
//! and adds the refunds, which only the payments know.

use std::collections::BTreeMap;

use hypha_core::{
    AuthError, ChannelId, Clock, Ctx, Currency, ErrorCode, MarketId, Money, OrderId, RepoError,
    Scope, Timestamp, Timezone,
};
use hypha_orders::{CurrencyTop, HasOrders, OrderRepo, ProductUnits, SalesQuery};
use hypha_payments::{HasPayments, PaymentRepo};
use jiff::{Span, Zoned, tz::TimeZone};
use thiserror::Error;

/// The most buckets a report has: a year and a day of days.
pub const MAX_BUCKETS: usize = 366;
/// How many products each top list holds unless asked: 5, at most 20.
pub const DEFAULT_TOP: u32 = 5;
pub const MAX_TOP: u32 = 20;
/// The period a report covers unless asked: the last 7 days, today included.
pub const DEFAULT_DAYS: i64 = 7;

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ReportError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error("the period must end after it starts")]
    InvalidPeriod,
    #[error("a report has at most {max} buckets: ask for a shorter period or a longer interval")]
    TooManyBuckets { max: usize },
    #[error("a top list holds 1 to {max} products")]
    TopOutOfRange { max: u32 },
}

impl ErrorCode for ReportError {
    fn code(&self) -> &'static str {
        match self {
            ReportError::Auth(e) => e.code(),
            ReportError::Repo(e) => e.code(),
            ReportError::InvalidPeriod => "invalid_period",
            ReportError::TooManyBuckets { .. } => "period_too_long",
            ReportError::TopOutOfRange { .. } => "invalid_top",
        }
    }
}

/// How long each bucket is, in the store's calendar. Weeks start on Monday.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Interval {
    #[default]
    Day,
    Week,
    Month,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SalesReportInput {
    /// The period's start; `DEFAULT_DAYS` before `to` when absent.
    pub from: Option<Timestamp>,
    /// The period's end (left out); the start of tomorrow in the store's zone when absent.
    pub to: Option<Timestamp>,
    pub interval: Interval,
    pub channel: Option<ChannelId>,
    pub market: Option<MarketId>,
    /// How many products each top list holds; `DEFAULT_TOP` when absent.
    pub top: Option<u32>,
    /// The store's time zone; UTC without one.
    pub zone: Option<Timezone>,
}

/// One currency's sales. `gross_sales` is the line totals (tax included where the
/// market's prices include it), `net` what the orders came to less what was given back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SalesRow {
    pub currency: Currency,
    pub orders_count: u64,
    pub gross_sales: Money,
    pub shipping: Money,
    pub taxes: Money,
    pub total: Money,
    /// Given back by the orders' payments so far, counted with the orders (when they
    /// were placed), since a payment keeps the sum of its refunds, not their dates.
    pub refunded: Money,
    pub net: Money,
}

/// A stretch of the period: the currencies sold in it, by code, and the orders cancelled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SalesBucket {
    pub start: Timestamp,
    pub end: Timestamp,
    pub sales: Vec<SalesRow>,
    pub cancelled_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SalesReport {
    pub from: Timestamp,
    pub to: Timestamp,
    pub interval: Interval,
    /// The zone the buckets follow.
    pub zone: Option<Timezone>,
    /// Every bucket, empty ones too; the first starts at `from`, the last ends at `to`.
    pub buckets: Vec<SalesBucket>,
    /// The whole period.
    pub total: SalesBucket,
    pub top_by_units: Vec<ProductUnits>,
    /// Per currency, by code.
    pub top_by_revenue: Vec<CurrencyTop>,
}

/// The report for `input`'s period, as of `clock`'s now.
pub async fn sales_report<S: HasOrders + HasPayments>(
    ctx: &Ctx,
    ports: &S,
    clock: &impl Clock,
    input: SalesReportInput,
) -> Result<SalesReport, ReportError> {
    ctx.require(Scope::OrdersRead)?;
    let top = input.top.unwrap_or(DEFAULT_TOP);
    if !(1..=MAX_TOP).contains(&top) {
        return Err(ReportError::TopOutOfRange { max: MAX_TOP });
    }
    let tz = time_zone(input.zone);
    let (from, to) = period(clock.now(), input.from, input.to, &tz)?;
    let starts = bucket_starts(from, to, input.interval, &tz)?;
    let query = SalesQuery {
        starts: starts.clone(),
        end: to,
        channel: input.channel,
        market: input.market,
        top,
    };
    let figures = ports.orders().sales(&query).await?;

    // Refunds by order, from its payments.
    let refunded_orders: Vec<OrderId> = figures.refunded.iter().map(|(o, _)| *o).collect();
    let payments = ports.payments().list_for_orders(&refunded_orders).await?;
    let mut refunds: BTreeMap<OrderId, (Currency, i128)> = BTreeMap::new();
    for payment in &payments {
        let (Some(order), refunded) = (payment.order(), payment.refunded()) else {
            continue;
        };
        refunds.entry(order).or_insert((refunded.currency(), 0)).1 += i128::from(refunded.amount());
    }

    let mut rows: Vec<Vec<Sums>> = figures
        .buckets
        .iter()
        .map(|b| b.sales.iter().map(Sums::of).collect())
        .collect();
    for (order, bucket) in &figures.refunded {
        let Some((currency, amount)) = refunds.get(order) else {
            continue;
        };
        // Counted orders always have their currency's row in their bucket.
        if let Some(row) = rows
            .get_mut(*bucket)
            .and_then(|r| r.iter_mut().find(|r| r.currency == *currency))
        {
            row.refunded += amount;
        }
    }

    let mut overall: BTreeMap<&'static str, Sums> = BTreeMap::new();
    for row in rows.iter().flatten() {
        overall
            .entry(row.currency.as_str())
            .or_insert_with(|| Sums::zero(row.currency))
            .add(row);
    }
    let buckets = starts
        .iter()
        .enumerate()
        .map(|(i, start)| SalesBucket {
            start: *start,
            end: starts.get(i + 1).copied().unwrap_or(to),
            sales: rows
                .get(i)
                .map_or_else(Vec::new, |r| r.iter().map(Sums::row).collect()),
            cancelled_count: figures.buckets.get(i).map_or(0, |b| b.cancelled_count),
        })
        .collect();
    Ok(SalesReport {
        from,
        to,
        interval: input.interval,
        zone: input.zone,
        buckets,
        total: SalesBucket {
            start: from,
            end: to,
            sales: overall.values().map(Sums::row).collect(),
            cancelled_count: figures.buckets.iter().map(|b| b.cancelled_count).sum(),
        },
        top_by_units: figures.top_by_units,
        top_by_revenue: figures.top_by_revenue,
    })
}

/// A row's amounts in `i128` while they're added up.
struct Sums {
    currency: Currency,
    orders_count: u64,
    gross_sales: i128,
    shipping: i128,
    taxes: i128,
    total: i128,
    refunded: i128,
}

impl Sums {
    fn zero(currency: Currency) -> Self {
        Self {
            currency,
            orders_count: 0,
            gross_sales: 0,
            shipping: 0,
            taxes: 0,
            total: 0,
            refunded: 0,
        }
    }

    fn of(s: &hypha_orders::CurrencySales) -> Self {
        Self {
            currency: s.currency,
            orders_count: s.orders_count,
            gross_sales: i128::from(s.subtotal.amount()),
            shipping: i128::from(s.shipping.amount()),
            taxes: i128::from(s.taxes.amount()),
            total: i128::from(s.total.amount()),
            refunded: 0,
        }
    }

    fn add(&mut self, other: &Sums) {
        self.orders_count += other.orders_count;
        self.gross_sales += other.gross_sales;
        self.shipping += other.shipping;
        self.taxes += other.taxes;
        self.total += other.total;
        self.refunded += other.refunded;
    }

    fn row(&self) -> SalesRow {
        let money = |amount: i128| bounded(amount, self.currency);
        SalesRow {
            currency: self.currency,
            orders_count: self.orders_count,
            gross_sales: money(self.gross_sales),
            shipping: money(self.shipping),
            taxes: money(self.taxes),
            total: money(self.total),
            refunded: money(self.refunded),
            net: money(self.total - self.refunded),
        }
    }
}

/// `amount` held within ±`Money::MAX_AMOUNT`: a report's sum shouldn't make it unreadable,
/// as for `OrderStats`.
fn bounded(amount: i128, currency: Currency) -> Money {
    let max = i128::from(Money::MAX_AMOUNT);
    let amount = i64::try_from(amount.clamp(-max, max)).unwrap_or(0);
    Money::new(amount, currency).unwrap_or_else(|_| Money::zero(currency))
}

/// The zone's rules from the bundled database (not the system's), so every host buckets
/// alike. A name it lacks (core's list is checked separately) reads as UTC.
fn time_zone(zone: Option<Timezone>) -> TimeZone {
    zone.and_then(|zone| {
        jiff::tz::TimeZoneDatabase::bundled()
            .get(zone.as_str())
            .ok()
    })
    .unwrap_or(TimeZone::UTC)
}

fn zoned(at: Timestamp, tz: &TimeZone) -> Result<Zoned, ReportError> {
    jiff::Timestamp::from_microsecond(at.unix_micros())
        .map(|t| t.to_zoned(tz.clone()))
        .map_err(|_| ReportError::InvalidPeriod)
}

fn instant(at: &Zoned) -> Timestamp {
    Timestamp::from_unix_micros(at.timestamp().as_microsecond())
}

/// `at` moved by `days` calendar days, at the start of that day.
fn days_later(at: &Zoned, days: i64) -> Result<Zoned, ReportError> {
    let span = Span::new()
        .try_days(days)
        .map_err(|_| ReportError::InvalidPeriod)?;
    at.checked_add(span)
        .and_then(|z| z.start_of_day())
        .map_err(|_| ReportError::InvalidPeriod)
}

/// The period asked for, with the defaults: up to the start of tomorrow, and
/// `DEFAULT_DAYS` back from its end.
fn period(
    now: Timestamp,
    from: Option<Timestamp>,
    to: Option<Timestamp>,
    tz: &TimeZone,
) -> Result<(Timestamp, Timestamp), ReportError> {
    let to = match to {
        Some(to) => to,
        None => {
            let today = zoned(now, tz)?
                .start_of_day()
                .map_err(|_| ReportError::InvalidPeriod)?;
            instant(&days_later(&today, 1)?)
        }
    };
    let from = match from {
        Some(from) => from,
        None => {
            let span = Span::new()
                .try_days(-DEFAULT_DAYS)
                .map_err(|_| ReportError::InvalidPeriod)?;
            let back = zoned(to, tz)?
                .checked_add(span)
                .map_err(|_| ReportError::InvalidPeriod)?;
            instant(&back)
        }
    };
    if from >= to {
        return Err(ReportError::InvalidPeriod);
    }
    Ok((from, to))
}

/// Where each bucket starts: `from`, then every start of a day, week (Monday) or month
/// in the zone before `to`.
fn bucket_starts(
    from: Timestamp,
    to: Timestamp,
    interval: Interval,
    tz: &TimeZone,
) -> Result<Vec<Timestamp>, ReportError> {
    let mut starts = vec![from];
    let mut at = zoned(from, tz)?;
    loop {
        let next = next_start(&at, interval)?;
        let start = instant(&next);
        if start >= to {
            return Ok(starts);
        }
        if starts.len() == MAX_BUCKETS {
            return Err(ReportError::TooManyBuckets { max: MAX_BUCKETS });
        }
        starts.push(start);
        at = next;
    }
}

/// The first start of an `interval` after `at`.
fn next_start(at: &Zoned, interval: Interval) -> Result<Zoned, ReportError> {
    let failed = |_| ReportError::InvalidPeriod;
    match interval {
        Interval::Day => days_later(&at.start_of_day().map_err(failed)?, 1),
        Interval::Week => {
            let monday = i64::from(at.weekday().to_monday_zero_offset());
            days_later(&at.start_of_day().map_err(failed)?, 7 - monday)
        }
        Interval::Month => {
            let first = at.first_of_month().map_err(failed)?;
            let span = Span::new().try_months(1).map_err(failed)?;
            first
                .checked_add(span)
                .and_then(|z| z.start_of_day())
                .map_err(failed)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tz(name: &str) -> TimeZone {
        time_zone(Some(Timezone::new(name).unwrap()))
    }

    fn at(rfc3339: &str) -> Timestamp {
        let t: jiff::Timestamp = rfc3339.parse().unwrap();
        Timestamp::from_unix_micros(t.as_microsecond())
    }

    fn starts(from: &str, to: &str, interval: Interval, zone: &str) -> Vec<Timestamp> {
        bucket_starts(at(from), at(to), interval, &tz(zone)).unwrap()
    }

    #[test]
    fn days_start_at_midnight_in_the_zone() {
        assert_eq!(
            starts(
                "2026-10-20T10:00:00Z",
                "2026-10-22T22:00:00Z",
                Interval::Day,
                "Europe/Belgrade"
            ),
            [
                at("2026-10-20T10:00:00Z"),
                at("2026-10-20T22:00:00Z"),
                at("2026-10-21T22:00:00Z"),
            ]
        );
    }

    #[test]
    fn a_day_across_a_clock_change_is_23_or_25_hours() {
        // Belgrade leaves summer time on 25 October 2026.
        assert_eq!(
            starts(
                "2026-10-24T22:00:00Z",
                "2026-10-26T23:00:00Z",
                Interval::Day,
                "Europe/Belgrade"
            ),
            [at("2026-10-24T22:00:00Z"), at("2026-10-25T23:00:00Z"),]
        );
    }

    #[test]
    fn weeks_start_on_monday_and_months_on_the_first() {
        // 22 October 2026 is a Thursday.
        assert_eq!(
            starts(
                "2026-10-22T00:00:00Z",
                "2026-11-03T00:00:00Z",
                Interval::Week,
                "UTC"
            ),
            [
                at("2026-10-22T00:00:00Z"),
                at("2026-10-26T00:00:00Z"),
                at("2026-11-02T00:00:00Z"),
            ]
        );
        assert_eq!(
            starts(
                "2026-01-31T12:00:00Z",
                "2026-03-01T00:00:00Z",
                Interval::Month,
                "UTC"
            ),
            [at("2026-01-31T12:00:00Z"), at("2026-02-01T00:00:00Z")]
        );
    }

    #[test]
    fn the_default_period_is_the_last_seven_days_today_included() {
        let now = at("2026-10-22T15:30:00Z");
        assert_eq!(
            period(now, None, None, &tz("Europe/Belgrade")),
            Ok((at("2026-10-15T22:00:00Z"), at("2026-10-22T22:00:00Z")))
        );
        assert_eq!(
            period(now, None, Some(at("2026-10-10T00:00:00Z")), &tz("UTC")),
            Ok((at("2026-10-03T00:00:00Z"), at("2026-10-10T00:00:00Z")))
        );
    }

    #[test]
    fn a_period_must_end_after_it_starts() {
        let now = at("2026-10-22T15:30:00Z");
        let t = at("2026-10-01T00:00:00Z");
        assert_eq!(
            period(now, Some(t), Some(t), &tz("UTC")),
            Err(ReportError::InvalidPeriod)
        );
    }

    #[test]
    fn a_report_has_at_most_366_buckets() {
        let utc = tz("UTC");
        let year = bucket_starts(
            at("2024-01-01T00:00:00Z"),
            at("2025-01-01T00:00:00Z"),
            Interval::Day,
            &utc,
        );
        assert_eq!(year.map(|s| s.len()), Ok(366));
        assert_eq!(
            bucket_starts(
                at("2024-01-01T00:00:00Z"),
                at("2025-01-02T00:00:00Z"),
                Interval::Day,
                &utc
            ),
            Err(ReportError::TooManyBuckets { max: 366 })
        );
    }

    #[test]
    fn amounts_stay_within_money() {
        let eur = Currency::EUR;
        assert_eq!(
            bounded(i128::from(Money::MAX_AMOUNT) * 2, eur).amount(),
            Money::MAX_AMOUNT
        );
        assert_eq!(bounded(-5, eur).amount(), -5);
    }
}
