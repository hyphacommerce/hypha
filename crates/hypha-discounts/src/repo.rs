use std::collections::{HashMap, HashSet};

use hypha_core::{CustomerId, DiscountId, Money, OrderId, Page, PageRequest, RepoError, Timestamp};

use crate::{
    code::DiscountCode,
    discount::{Class, Discount, Method, Phase},
};

/// Uniqueness rules the discounts repo enforces, reported as `RepoError::Duplicate(key)`.
pub mod keys {
    pub const DISCOUNT_ID: &str = "discount_id";
    /// A code's normalised text is unique in the store.
    pub const DISCOUNT_CODE: &str = "discount_code";
    /// Claiming a use when the discount's or the code's limit is reached (or the discount
    /// is gone): the counter didn't move.
    pub const DISCOUNT_USAGE_LIMIT: &str = "discount_usage_limit";
    /// A second order of one customer for a once-per-customer discount.
    pub const DISCOUNT_PER_CUSTOMER: &str = "discount_per_customer";
}

/// Which discounts a list holds; every field narrows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscountFilter {
    /// Where they stand at `at`.
    pub phase: Option<Phase>,
    pub method: Option<Method>,
    /// Those with a reward of this class.
    pub class: Option<Class>,
    /// In the title (ignoring case) or a code (normalised, as a part).
    pub q: Option<String>,
    /// The discount of this code (normalised, exact).
    pub code: Option<String>,
    /// The moment `phase` is judged at.
    pub at: Timestamp,
}

impl DiscountFilter {
    pub fn everything(at: Timestamp) -> Self {
        Self {
            phase: None,
            method: None,
            class: None,
            q: None,
            code: None,
            at,
        }
    }
}

/// Which of a discount's codes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CodeFilter {
    /// A part of the normalised code.
    pub q: Option<String>,
    pub batch: Option<String>,
}

/// Codes to remove: listed ones (normalised), or a whole batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodeSelection {
    Codes(Vec<String>),
    Batch(String),
}

/// One order a discount was used on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Usage {
    pub discount: DiscountId,
    pub order: OrderId,
    pub customer: CustomerId,
    /// The code it came by, normalised.
    pub code: Option<String>,
    /// What the discount took off the order.
    pub amount: Money,
    pub at: Timestamp,
}

/// A use checkout claims in the order's batch. The discount's counter moves only while
/// it's under `limit`, and the code's while under its own; a once-per-customer use is
/// unique per customer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageClaim {
    pub discount: DiscountId,
    pub limit: Option<u32>,
    /// The code (normalised) and its own limit.
    pub code: Option<(String, Option<u32>)>,
    pub order: OrderId,
    pub customer: CustomerId,
    pub once_per_customer: bool,
    /// What the discount takes off the order.
    pub amount: Money,
    pub at: Timestamp,
}

/// Discounts, their codes and their usage.
#[allow(async_fn_in_trait)]
pub trait DiscountRepo {
    async fn get(&self, id: DiscountId) -> Result<Option<Discount>, RepoError>;
    /// Those of `ids` that exist, by ID.
    async fn get_many(&self, ids: &[DiscountId]) -> Result<Vec<Discount>, RepoError>;
    /// A page of what `filter` matches, newest first.
    async fn list(
        &self,
        filter: &DiscountFilter,
        page: PageRequest<DiscountId>,
    ) -> Result<Page<Discount>, RepoError>;
    /// The automatic discounts live at `at` (active, inside their window), by ID: what
    /// every cart weighs.
    async fn live_automatic(&self, at: Timestamp) -> Result<Vec<Discount>, RepoError>;
    /// How many automatic discounts are active and not expired at `at`.
    async fn count_live_automatic(&self, at: Timestamp) -> Result<u64, RepoError>;
    /// Stores a new discount (version 1) with its usage counter at zero. Enforces
    /// `DISCOUNT_ID`.
    async fn insert(&self, discount: &mut Discount) -> Result<(), RepoError>;
    /// Stores changes and bumps the version, or fails with `StaleVersion`. Never touches
    /// the usage counter.
    async fn update(&self, discount: &mut Discount) -> Result<(), RepoError>;
    /// Removes it with its codes, counter and usages, writing its events, when still at
    /// the version loaded; otherwise `StaleVersion`.
    async fn delete(&self, discount: &mut Discount) -> Result<(), RepoError>;

    /// The codes among `normalized` that exist (any discount's, active or not).
    async fn find_codes(&self, normalized: &[String]) -> Result<Vec<DiscountCode>, RepoError>;
    /// A page of the discount's codes, by normalised code; `page.after` is the last code
    /// seen.
    async fn code_page(
        &self,
        discount: DiscountId,
        filter: &CodeFilter,
        page: PageRequest<String>,
    ) -> Result<Page<DiscountCode>, RepoError>;
    /// How many codes `filter` matches.
    async fn count_codes(
        &self,
        discount: DiscountId,
        filter: &CodeFilter,
    ) -> Result<u64, RepoError>;
    /// How many codes each of `discounts` has; one without any is left out.
    async fn code_counts(
        &self,
        discounts: &[DiscountId],
    ) -> Result<HashMap<DiscountId, u64>, RepoError>;
    /// Adds codes, unused. Enforces `DISCOUNT_CODE` (all or none are added). Records no
    /// event: commands stage it (`StageDiscounts`) with `discount.codes_added`; this is
    /// for setting up data.
    ///
    /// Precondition: their discount exists; otherwise adapters fail with `Backend`.
    async fn add_codes(&self, codes: &[DiscountCode]) -> Result<(), RepoError>;

    /// Orders each of `discounts` was used on; one never used is left out.
    async fn usage_counts(
        &self,
        discounts: &[DiscountId],
    ) -> Result<HashMap<DiscountId, u64>, RepoError>;
    /// Which of `discounts` `customer` used.
    async fn used_by(
        &self,
        discounts: &[DiscountId],
        customer: CustomerId,
    ) -> Result<HashSet<DiscountId>, RepoError>;
    /// A page of the orders it was used on, newest order first.
    async fn usage_page(
        &self,
        discount: DiscountId,
        page: PageRequest<OrderId>,
    ) -> Result<Page<Usage>, RepoError>;
}

/// Stages discounts, their codes and their usage in a `hypha_core::HasBatch` batch.
pub trait StageDiscounts {
    fn insert_discount(&mut self, discount: &mut Discount);
    /// Precondition: the discount exists (or is inserted in the same batch).
    fn add_discount_codes(&mut self, codes: &[DiscountCode]);
    fn remove_discount_codes(&mut self, discount: DiscountId, codes: CodeSelection);
    /// Fails the commit with `Duplicate(DISCOUNT_USAGE_LIMIT)` when a counter is at its
    /// limit (or the discount or code is gone), `Duplicate(DISCOUNT_PER_CUSTOMER)` when the
    /// customer used it already. Precondition: the order and customer are written in the
    /// same batch or exist.
    fn claim_discount_usage(&mut self, claim: UsageClaim);
}

pub trait HasDiscounts {
    type Discounts: DiscountRepo;
    fn discounts(&self) -> &Self::Discounts;
}

impl<T: HasDiscounts + ?Sized> HasDiscounts for &T {
    type Discounts = T::Discounts;
    fn discounts(&self) -> &Self::Discounts {
        (**self).discounts()
    }
}
