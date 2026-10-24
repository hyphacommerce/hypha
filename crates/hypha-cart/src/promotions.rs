//! The discounts a cart weighs, behind a port the cart defines: the cart hands over its
//! priced lines and shipping, and gets back what comes off each and where each code
//! stands. What a discount is stays on the other side (`hypha-promotions`).

use hypha_core::{
    ChannelId, Currency, CustomerGroupId, DiscountId, MarketId, Money, ProductId, RepoError,
    Timestamp, VariantId,
};

use crate::Cart;

/// Whatever weighs a cart's discounts for these ports: a host's per-request ports, a test
/// world.
pub trait HasPromotions {
    type Promotions: Promotions<Self>;
}

/// Reads what a cart's discounts are weighed with, from the ports `P`.
#[allow(async_fn_in_trait)]
pub trait Promotions<P: ?Sized> {
    /// Read once per pricing; the default weighs nothing (no discount applies).
    type Reading: PromotionReading + Default;

    /// `raw` as a cart keeps codes; `None` when it can't be a code at all.
    fn normalize_code(raw: &str) -> Option<String>;

    /// Whether `code` (normalised) is active and its discount live at `at`: what a cart
    /// may take in.
    async fn code_is_live(ports: &P, code: &str, at: Timestamp) -> Result<bool, RepoError>;

    /// The discounts that may apply to `cart` at `at`, for a buyer in `groups`.
    async fn read(
        ports: &P,
        cart: &Cart,
        groups: Vec<CustomerGroupId>,
        at: Timestamp,
    ) -> Result<Self::Reading, RepoError>;
}

/// What a cart's discounts were read into: applied to its lines with no I/O, as often as
/// pricing and the shipping offers need.
pub trait PromotionReading {
    fn apply(&self, input: &PromotionInput<'_>) -> PromotionOutcome;
    /// Whether a discount weighed may reduce shipping: without one, offers skip `apply`.
    fn reduces_shipping(&self) -> bool;
}

/// A line that can be bought, priced before discounts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromotionLine {
    pub product: ProductId,
    pub variant: VariantId,
    pub unit_price: Money,
    pub quantity: u32,
    /// Shown against a compare-at price.
    pub on_sale: bool,
}

/// A group of lines shipped together, with the chosen option's price.
pub struct PromotionShipping<'a> {
    /// The input lines it ships (their indexes).
    pub lines: Vec<usize>,
    /// The chosen option's price for a subtotal of its lines after item and order
    /// discounts; `None` when nothing is chosen or it can't be priced.
    pub rate: &'a dyn Fn(Money) -> Option<Money>,
}

/// What the discounts apply to.
pub struct PromotionInput<'a> {
    pub market: MarketId,
    pub channel: ChannelId,
    pub currency: Currency,
    /// The lines that can be bought, in cart order.
    pub lines: Vec<PromotionLine>,
    pub shipping: Vec<PromotionShipping<'a>>,
    /// The codes the cart holds, in the order entered.
    pub codes: &'a [String],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromotionOutcome {
    /// Item rewards, then order, then shipping.
    pub applications: Vec<Application>,
    /// Per input line, its shares.
    pub lines: Vec<Vec<Share>>,
    /// Per input shipping group.
    pub shipping: Vec<ShippingShares>,
    /// Per code held, in order.
    pub codes: Vec<CodeOutcome>,
    /// Everything taken off: the sum of the applications.
    pub total: Money,
}

/// A shipping group after discounts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShippingShares {
    /// Its lines' amount after item and order discounts: what the rate was priced for.
    pub subtotal: Money,
    pub shares: Vec<Share>,
}

/// What a discount reduces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Product,
    Order,
    Shipping,
}

/// One reward applied: what it was and how much it took in all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Application {
    pub discount: DiscountId,
    /// The code it came by, normalised.
    pub code: Option<String>,
    pub title: String,
    /// One sentence naming what it applies to, in the cart's market.
    pub summary: String,
    pub class: Class,
    /// The reward's value and target in the discounts' JSON form, kept as is by orders.
    pub value: serde_json::Value,
    pub target: serde_json::Value,
    pub amount: Money,
    /// What checkout claims its use under: the buyer's one use, and its code's own limit.
    pub once_per_customer: bool,
    pub code_limit: Option<u32>,
}

/// Part of an application on one line or shipping group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Share {
    /// Its index in the applications.
    pub application: usize,
    pub amount: Money,
    /// The units it covered, for a buy X get Y discount.
    pub units: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeOutcome {
    pub code: String,
    pub state: CodeState,
}

/// What's missing for a minimum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Needed {
    /// This much more of the qualifying items.
    Amount(Money),
    /// This many more qualifying units.
    Quantity(u32),
}

/// Where a code the cart holds stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeState {
    /// In the discounts applied.
    Applied,
    /// A condition fails (`markets`, `channels`, `customer_groups`, `customers`,
    /// `signed_in`).
    NotEligible { condition: &'static str },
    /// The qualifying items fall short.
    MinimumNotMet { needed: Needed },
    /// Nothing in the cart is reduced by it.
    NoMatchingItems,
    /// A combination that saves more leaves it out; `discount` is one that excludes it.
    BlockedByOther { discount: DiscountId },
    /// Applying it wouldn't lower what the buyer pays, so it's left out.
    NoSaving,
    /// Its uses are taken (in all, the code's own, or once per customer).
    LimitReached,
    /// It has no amount for the cart's market.
    NotAvailableInMarket,
    /// Its discount's end date passed.
    Expired,
    /// The code or its discount is gone, deactivated, a draft, or not started yet.
    Invalid,
}

impl CodeState {
    pub fn name(self) -> &'static str {
        match self {
            CodeState::Applied => "applied",
            CodeState::NotEligible { .. } => "not_eligible",
            CodeState::MinimumNotMet { .. } => "minimum_not_met",
            CodeState::NoMatchingItems => "no_matching_items",
            CodeState::BlockedByOther { .. } => "blocked_by_other",
            CodeState::NoSaving => "no_saving",
            CodeState::LimitReached => "limit_reached",
            CodeState::NotAvailableInMarket => "not_available_in_market",
            CodeState::Expired => "expired",
            CodeState::Invalid => "invalid",
        }
    }
}

/// What a cart's ports weigh its discounts with.
pub type ReadingOf<S> = <<S as HasPromotions>::Promotions as Promotions<S>>::Reading;
