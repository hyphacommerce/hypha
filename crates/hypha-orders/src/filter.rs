//! What an order list is narrowed to and how it is ordered (design note 0024, §2).

use hypha_core::{
    ChannelId, CountryCode, Currency, CustomerId, Direction, FieldFilter, SortKey, Timestamp,
    optional_text,
};

use crate::{FinancialStatus, FulfillmentStatus, Order, OrderError, OrderSummary};

/// An order list's filters, checked. Every one given must match; a list of statuses
/// matches any of them, and an empty one every status.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OrderFilter {
    q: Option<String>,
    financial_statuses: Vec<FinancialStatus>,
    fulfillment_statuses: Vec<FulfillmentStatus>,
    cancelled: Option<bool>,
    customer: Option<CustomerId>,
    channel: Option<ChannelId>,
    country: Option<CountryCode>,
    currency: Option<Currency>,
    placed_from: Option<Timestamp>,
    placed_before: Option<Timestamp>,
    fields: Vec<FieldFilter>,
}

/// What an order list may be narrowed to, unchecked (`OrderFilter::new` checks it).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OrderFilterInput {
    /// The order's number (`1001`, or `#1001`), or text its email contains, ignoring case.
    pub q: Option<String>,
    pub financial_statuses: Vec<FinancialStatus>,
    pub fulfillment_statuses: Vec<FulfillmentStatus>,
    /// Only cancelled orders (`true`) or only those that aren't (`false`).
    pub cancelled: Option<bool>,
    /// Only this customer's orders (design note 0032, §12 step 1).
    pub customer: Option<CustomerId>,
    pub channel: Option<ChannelId>,
    pub country: Option<CountryCode>,
    pub currency: Option<Currency>,
    /// Placed at or after this instant.
    pub placed_from: Option<Timestamp>,
    /// Placed before this instant.
    pub placed_before: Option<Timestamp>,
}

impl OrderFilter {
    /// `q` follows the shared text rule (trimmed, at most 255 characters); blank is no
    /// filter.
    pub fn new(input: OrderFilterInput) -> Result<Self, OrderError> {
        let mut financial_statuses = input.financial_statuses;
        financial_statuses.sort_by_key(|s| s.as_str());
        financial_statuses.dedup();
        let mut fulfillment_statuses = input.fulfillment_statuses;
        fulfillment_statuses.sort_by_key(|s| s.as_str());
        fulfillment_statuses.dedup();
        Ok(Self {
            q: optional_text("q", input.q).map_err(OrderError::Filter)?,
            financial_statuses,
            fulfillment_statuses,
            cancelled: input.cancelled,
            customer: input.customer,
            channel: input.channel,
            country: input.country,
            currency: input.currency,
            placed_from: input.placed_from,
            placed_before: input.placed_before,
            fields: Vec::new(),
        })
    }

    /// Filters on custom fields (design note 0033, §7), checked against their definitions
    /// by `hypha-custom-fields` already; every one must match. `matches` doesn't see
    /// them: adapters apply them to the stored values.
    pub fn with_fields(mut self, fields: Vec<FieldFilter>) -> Self {
        self.fields = fields;
        self
    }

    pub fn fields(&self) -> &[FieldFilter] {
        &self.fields
    }

    /// The text filter as given, trimmed.
    pub fn q(&self) -> Option<&str> {
        self.q.as_deref()
    }

    /// The order number the text filter names, when it is one (`1001`, `#1001`).
    pub fn q_number(&self) -> Option<i64> {
        let q = self.q.as_deref()?;
        q.strip_prefix('#').unwrap_or(q).parse().ok()
    }

    pub fn financial_statuses(&self) -> &[FinancialStatus] {
        &self.financial_statuses
    }

    pub fn fulfillment_statuses(&self) -> &[FulfillmentStatus] {
        &self.fulfillment_statuses
    }

    pub fn cancelled(&self) -> Option<bool> {
        self.cancelled
    }

    pub fn customer(&self) -> Option<CustomerId> {
        self.customer
    }

    pub fn channel(&self) -> Option<ChannelId> {
        self.channel
    }

    pub fn country(&self) -> Option<CountryCode> {
        self.country
    }

    pub fn currency(&self) -> Option<Currency> {
        self.currency
    }

    pub fn placed_from(&self) -> Option<Timestamp> {
        self.placed_from
    }

    pub fn placed_before(&self) -> Option<Timestamp> {
        self.placed_before
    }

    /// Whether `order` is in the list. Adapters that filter in storage must agree (the
    /// repository contract checks).
    pub fn matches(&self, order: &Order) -> bool {
        let text = self.q.as_ref().is_none_or(|q| {
            self.q_number() == Some(order.number().value())
                || order
                    .email()
                    .as_str()
                    .to_lowercase()
                    .contains(&q.to_lowercase())
        });
        text && (self.financial_statuses.is_empty()
            || self.financial_statuses.contains(&order.financial_status()))
            && (self.fulfillment_statuses.is_empty()
                || self
                    .fulfillment_statuses
                    .contains(&order.fulfillment_status()))
            && self
                .cancelled
                .is_none_or(|c| c == order.cancelled().is_some())
            && self.customer.is_none_or(|c| c == order.customer())
            && self.channel.is_none_or(|c| c == order.channel())
            && self.country.is_none_or(|c| c == order.country())
            && self.currency.is_none_or(|c| c == order.currency())
            && self.placed_from.is_none_or(|t| order.placed_at() >= t)
            && self.placed_before.is_none_or(|t| order.placed_at() < t)
    }
}

/// How an order list is ordered (design note 0024, AU1); ties go by ID in the same
/// direction. Totals compare only within one currency, so sorting by them needs the
/// `currency` filter.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OrderSort {
    /// Newest first: placing order, as lists were before sorting.
    #[default]
    Newest,
    Oldest,
    TotalDesc,
    TotalAsc,
}

impl OrderSort {
    pub fn direction(self) -> Direction {
        match self {
            OrderSort::Oldest | OrderSort::TotalAsc => Direction::Ascending,
            OrderSort::Newest | OrderSort::TotalDesc => Direction::Descending,
        }
    }

    pub fn key(self, summary: &OrderSummary) -> SortKey {
        match self {
            OrderSort::Newest | OrderSort::Oldest => SortKey::Id,
            OrderSort::TotalDesc | OrderSort::TotalAsc => SortKey::Int(summary.total.amount()),
        }
    }

    /// Whether `key` is the kind this sort uses: a cursor made for another sort doesn't.
    pub fn fits(self, key: &SortKey) -> bool {
        matches!(
            (self, key),
            (OrderSort::Newest | OrderSort::Oldest, SortKey::Id)
                | (OrderSort::TotalDesc | OrderSort::TotalAsc, SortKey::Int(_))
        )
    }

    /// Refuses a sort `filter` can't give: totals without one currency.
    pub fn check(self, filter: &OrderFilter) -> Result<(), OrderError> {
        match self {
            OrderSort::TotalDesc | OrderSort::TotalAsc if filter.currency().is_none() => {
                Err(OrderError::SortNeedsCurrency)
            }
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use hypha_core::TextError;

    use super::*;

    fn filter(input: OrderFilterInput) -> OrderFilter {
        OrderFilter::new(input).unwrap()
    }

    #[test]
    fn a_number_is_read_with_or_without_a_hash() {
        let by = |q: &str| {
            filter(OrderFilterInput {
                q: Some(q.to_string()),
                ..OrderFilterInput::default()
            })
            .q_number()
        };
        assert_eq!(by("1001"), Some(1001));
        assert_eq!(by(" #1001 "), Some(1001));
        assert_eq!(by("ana@example.com"), None);
    }

    #[test]
    fn text_follows_the_text_rule() {
        assert_eq!(
            OrderFilter::new(OrderFilterInput {
                q: Some("x".repeat(256)),
                ..OrderFilterInput::default()
            }),
            Err(OrderError::Filter(TextError::TooLong("q")))
        );
        assert_eq!(
            OrderFilter::new(OrderFilterInput {
                q: Some(" ".to_string()),
                ..OrderFilterInput::default()
            }),
            Ok(OrderFilter::default())
        );
    }

    #[test]
    fn totals_sort_only_within_a_currency() {
        let any = OrderFilter::default();
        assert_eq!(
            OrderSort::TotalDesc.check(&any),
            Err(OrderError::SortNeedsCurrency)
        );
        assert_eq!(OrderSort::Newest.check(&any), Ok(()));
        let eur = filter(OrderFilterInput {
            currency: Some(Currency::EUR),
            ..OrderFilterInput::default()
        });
        assert_eq!(OrderSort::TotalAsc.check(&eur), Ok(()));
    }

    #[test]
    fn each_sort_fits_only_its_own_key() {
        assert!(OrderSort::Oldest.fits(&SortKey::Id));
        assert!(OrderSort::TotalAsc.fits(&SortKey::Int(5)));
        assert!(!OrderSort::TotalAsc.fits(&SortKey::Id));
        assert!(!OrderSort::Newest.fits(&SortKey::Int(5)));
    }
}
