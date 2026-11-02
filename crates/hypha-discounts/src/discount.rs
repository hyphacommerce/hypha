//! The discount aggregate: what it takes off, of which items, for whom, when, and how
//! often. The values inside it (conditions, rewards, kinds) are plain data; the discount
//! checks them as a whole when it's made, changed or restored, and reports every broken
//! rule at once with its path.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use hypha_core::{
    BasisPoints, CategoryId, ChannelId, Currency, CustomerGroupId, CustomerId, DiscountId,
    MarketId, Money, PendingEvent, ProductId, Recorded, Stamped, Stamps, Subject, Timestamp,
    VariantId, Version, Versioned, required_text,
};

use crate::{
    error::{DiscountError, Problem},
    events::{DISCOUNT, DiscountCreated, DiscountDeleted, DiscountUpdated},
};

/// Most IDs one condition or item set lists. Categories count with what's below them,
/// as for price lists, without being listed.
pub const MAX_IDS: usize = 1000;
/// Most rewards a standard discount has: one per class.
pub const MAX_REWARDS: usize = 3;
/// Most tiers a tiered discount has.
pub const MAX_TIERS: usize = 10;
/// Most automatic discounts that are active and not expired at once. Every cart read
/// weighs each of them, so this is a performance bound, not a product limit.
pub const MAX_LIVE_AUTOMATIC: usize = 100;

/// How a discount reaches a cart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Method {
    /// Applies by itself whenever its conditions hold.
    Automatic,
    /// Applies only to carts that entered one of its codes.
    Codes,
}

impl Method {
    pub fn as_str(self) -> &'static str {
        match self {
            Method::Automatic => "automatic",
            Method::Codes => "codes",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "automatic" => Some(Method::Automatic),
            "codes" => Some(Method::Codes),
            _ => None,
        }
    }
}

/// Whether a discount may apply at all. A draft never does, whatever its window says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Status {
    Draft,
    Active,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Draft => "draft",
            Status::Active => "active",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "draft" => Some(Status::Draft),
            "active" => Some(Status::Active),
            _ => None,
        }
    }
}

/// Where a discount stands at a moment, from its status and window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Phase {
    Draft,
    /// Active, but its window hasn't opened yet.
    Scheduled,
    Active,
    /// Active, but its window has closed.
    Expired,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::Draft => "draft",
            Phase::Scheduled => "scheduled",
            Phase::Active => "active",
            Phase::Expired => "expired",
        }
    }
}

/// A combination category: what a reward reduces. It decides which discounts may apply
/// together, never the value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Class {
    /// Item lines.
    Product,
    /// The subtotal after product discounts.
    Order,
    /// The chosen shipping.
    Shipping,
}

impl Class {
    pub const ALL: [Class; 3] = [Class::Product, Class::Order, Class::Shipping];

    pub fn as_str(self) -> &'static str {
        match self {
            Class::Product => "product",
            Class::Order => "order",
            Class::Shipping => "shipping",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Class::ALL.into_iter().find(|c| c.as_str() == s)
    }
}

/// A set of classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ClassSet(u8);

impl ClassSet {
    pub const NONE: ClassSet = ClassSet(0);
    pub const ALL: ClassSet = ClassSet(0b111);

    fn bit(class: Class) -> u8 {
        match class {
            Class::Product => 1,
            Class::Order => 2,
            Class::Shipping => 4,
        }
    }

    pub fn of(classes: &[Class]) -> Self {
        let mut set = ClassSet::NONE;
        for c in classes {
            set.insert(*c);
        }
        set
    }

    pub fn insert(&mut self, class: Class) {
        self.0 |= Self::bit(class);
    }

    pub fn contains(self, class: Class) -> bool {
        self.0 & Self::bit(class) != 0
    }

    /// Whether every class of `other` is in this set.
    pub fn covers(self, other: ClassSet) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// In `Class::ALL` order.
    pub fn classes(self) -> Vec<Class> {
        Class::ALL
            .into_iter()
            .filter(|c| self.contains(*c))
            .collect()
    }
}

/// An amount per market, each in that market's currency: what fixed amounts, minimums and
/// caps are given in. A market without an entry gets none.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct PerMarket(BTreeMap<MarketId, Money>);

impl PerMarket {
    pub fn new(amounts: BTreeMap<MarketId, Money>) -> Self {
        Self(amounts)
    }

    pub fn of(market: MarketId, amount: Money) -> Self {
        Self(BTreeMap::from([(market, amount)]))
    }

    pub fn get(&self, market: MarketId) -> Option<Money> {
        self.0.get(&market).copied()
    }

    /// By market ID.
    pub fn iter(&self) -> impl Iterator<Item = (MarketId, Money)> + '_ {
        self.0.iter().map(|(m, a)| (*m, *a))
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The same amounts without `market`'s (a market that's gone).
    pub fn without(&self, market: MarketId) -> Self {
        let mut amounts = self.0.clone();
        amounts.remove(&market);
        Self(amounts)
    }
}

/// Who and what a discount is for. All of a discount's conditions must hold; inside one,
/// any of its values will do.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Condition {
    Markets(Vec<MarketId>),
    Channels(Vec<ChannelId>),
    /// The buyer is in any of them.
    CustomerGroups(Vec<CustomerGroupId>),
    Customers(Vec<CustomerId>),
    /// The cart belongs to a customer (signed in, or linked by staff).
    SignedIn,
    /// The qualifying items cost at least this much in the cart's market.
    MinSubtotal(PerMarket),
    /// The cart holds at least this many qualifying units.
    MinQuantity(u32),
}

impl Condition {
    /// The condition's name in the API and in a code's `not_eligible` state.
    pub fn name(&self) -> &'static str {
        match self {
            Condition::Markets(_) => "markets",
            Condition::Channels(_) => "channels",
            Condition::CustomerGroups(_) => "customer_groups",
            Condition::Customers(_) => "customers",
            Condition::SignedIn => "signed_in",
            Condition::MinSubtotal(_) => "min_subtotal",
            Condition::MinQuantity(_) => "min_quantity",
        }
    }

    /// Whether it depends only on who and where the buyer is, not on the cart's items.
    pub fn is_about_buyer(&self) -> bool {
        !matches!(self, Condition::MinSubtotal(_) | Condition::MinQuantity(_))
    }
}

/// Which items.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Items {
    All,
    Products(Vec<ProductId>),
    Variants(Vec<VariantId>),
    /// The products in these categories or any category below them.
    Categories(Vec<CategoryId>),
}

impl Items {
    pub fn name(&self) -> &'static str {
        match self {
            Items::All => "all",
            Items::Products(_) => "products",
            Items::Variants(_) => "variants",
            Items::Categories(_) => "categories",
        }
    }
}

/// A set of items: the listed ones, optionally leaving out those on sale (a compare-at
/// price shown: "not on items already reduced").
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ItemSet {
    pub items: Items,
    pub exclude_on_sale: bool,
}

impl ItemSet {
    pub fn all() -> Self {
        Self {
            items: Items::All,
            exclude_on_sale: false,
        }
    }

    /// Whether a line of `variant` (of `product`, in `categories` with their ancestors,
    /// `on_sale` or not) is in the set.
    pub fn contains(
        &self,
        product: ProductId,
        variant: VariantId,
        categories: &HashSet<CategoryId>,
        on_sale: bool,
    ) -> bool {
        if self.exclude_on_sale && on_sale {
            return false;
        }
        match &self.items {
            Items::All => true,
            Items::Products(ids) => ids.contains(&product),
            Items::Variants(ids) => ids.contains(&variant),
            Items::Categories(ids) => ids.iter().any(|c| categories.contains(c)),
        }
    }

    /// Whether it may reach `product`, whatever its variant or sale: for showing a
    /// promotion on a product's page.
    pub fn may_reach(&self, product: ProductId, categories: &HashSet<CategoryId>) -> bool {
        match &self.items {
            Items::All | Items::Variants(_) => true,
            Items::Products(ids) => ids.contains(&product),
            Items::Categories(ids) => ids.iter().any(|c| categories.contains(c)),
        }
    }
}

/// How an amount off is spread over the items it reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Allocation {
    /// Off every unit.
    Each,
    /// Once, split over the items in proportion to their amounts.
    Across,
    /// Off each of the cheapest `units` units.
    Cheapest { units: u32 },
}

impl Allocation {
    pub fn name(self) -> &'static str {
        match self {
            Allocation::Each => "each",
            Allocation::Across => "across",
            Allocation::Cheapest { .. } => "cheapest",
        }
    }
}

/// How much a reward takes off.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Value {
    /// 1 to 10000 basis points (10000 is everything: free).
    Percent(BasisPoints),
    Amount {
        amounts: PerMarket,
        allocation: Allocation,
    },
    /// Items only: each unit costs this, when that's lower than its price.
    FixedPrice(PerMarket),
}

impl Value {
    pub fn name(&self) -> &'static str {
        match self {
            Value::Percent(_) => "percent",
            Value::Amount { .. } => "amount",
            Value::FixedPrice(_) => "fixed_price",
        }
    }

    /// The amounts it needs per market, if any.
    pub fn amounts(&self) -> Option<&PerMarket> {
        match self {
            Value::Percent(_) => None,
            Value::Amount { amounts, .. } | Value::FixedPrice(amounts) => Some(amounts),
        }
    }
}

/// What a reward reduces.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Target {
    Items(ItemSet),
    /// The subtotal after product discounts.
    Subtotal,
    /// The chosen shipping, at most `max_amount` off when given.
    Shipping {
        max_amount: Option<PerMarket>,
    },
}

impl Target {
    pub fn class(&self) -> Class {
        match self {
            Target::Items(_) => Class::Product,
            Target::Subtotal => Class::Order,
            Target::Shipping { .. } => Class::Shipping,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Target::Items(_) => "items",
            Target::Subtotal => "subtotal",
            Target::Shipping { .. } => "shipping",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Reward {
    pub target: Target,
    pub value: Value,
}

/// What a buy X get Y discount needs bought first.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Threshold {
    /// At least this many units.
    Quantity(u32),
    /// Units worth at least this much (at their prices before discounts).
    Amount(PerMarket),
}

/// What a tiered discount measures its items by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Measure {
    Quantity,
    Subtotal,
}

impl Measure {
    pub fn as_str(self) -> &'static str {
        match self {
            Measure::Quantity => "quantity",
            Measure::Subtotal => "subtotal",
        }
    }
}

/// From `threshold` (a quantity, or an amount per market, as the measure says), `value`
/// off the items.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Tier {
    pub threshold: Threshold,
    /// A percentage or an amount (spread across the items).
    pub value: Value,
}

/// The shape of a discount.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Kind {
    /// One to three rewards, one per class: "20% off cultures and free shipping".
    Standard { rewards: Vec<Reward> },
    /// Buy `buy` of `buys`, get `get_quantity` units of `gets` at `value` off, repeated
    /// while the cart qualifies, up to `uses_per_order` times.
    BuyXGetY {
        buys: ItemSet,
        buy: Threshold,
        gets: ItemSet,
        get_quantity: u32,
        /// A percentage (10000 is free) or an amount off each unit got.
        value: Value,
        uses_per_order: Option<u32>,
    },
    /// The highest tier the items reach applies to them.
    Tiered {
        items: ItemSet,
        measure: Measure,
        /// By threshold, ascending.
        tiers: Vec<Tier>,
    },
}

impl Kind {
    pub fn name(&self) -> &'static str {
        match self {
            Kind::Standard { .. } => "standard",
            Kind::BuyXGetY { .. } => "buy_x_get_y",
            Kind::Tiered { .. } => "tiered",
        }
    }

    /// The classes its rewards reduce: a standard discount's targets', and product for the
    /// others (they reduce item lines).
    pub fn classes(&self) -> ClassSet {
        match self {
            Kind::Standard { rewards } => {
                let classes: Vec<Class> = rewards.iter().map(|r| r.target.class()).collect();
                ClassSet::of(&classes)
            }
            Kind::BuyXGetY { .. } | Kind::Tiered { .. } => ClassSet::of(&[Class::Product]),
        }
    }

    /// Every amount per market it holds, with the path it's at: for checking currencies
    /// and markets.
    fn amounts(&self) -> Vec<(String, &PerMarket)> {
        let mut found = Vec::new();
        match self {
            Kind::Standard { rewards } => {
                for (i, reward) in rewards.iter().enumerate() {
                    if let Target::Shipping {
                        max_amount: Some(max),
                    } = &reward.target
                    {
                        found.push((format!("kind.rewards[{i}].target.max_amount"), max));
                    }
                    if let Some(amounts) = reward.value.amounts() {
                        found.push((format!("kind.rewards[{i}].value.amounts"), amounts));
                    }
                }
            }
            Kind::BuyXGetY { buy, value, .. } => {
                if let Threshold::Amount(amounts) = buy {
                    found.push(("kind.buy.amounts".to_string(), amounts));
                }
                if let Some(amounts) = value.amounts() {
                    found.push(("kind.value.amounts".to_string(), amounts));
                }
            }
            Kind::Tiered { tiers, .. } => {
                for (i, tier) in tiers.iter().enumerate() {
                    if let Threshold::Amount(amounts) = &tier.threshold {
                        found.push((format!("kind.tiers[{i}].threshold.amounts"), amounts));
                    }
                    if let Some(amounts) = tier.value.amounts() {
                        found.push((format!("kind.tiers[{i}].value.amounts"), amounts));
                    }
                }
            }
        }
        found
    }
}

/// How often a discount may be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Limits {
    /// Orders it may apply to in all; `None` for no limit.
    pub total: Option<u32>,
    /// At most one order per customer (a guest is counted by their email's customer).
    pub once_per_customer: bool,
}

/// A discount as a caller describes it; `Discount::new` and `Discount::update` check it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscountDraft {
    /// Shown to buyers ("Welcome offer").
    pub title: String,
    pub method: Method,
    pub status: Status,
    pub starts_at: Option<Timestamp>,
    pub ends_at: Option<Timestamp>,
    pub conditions: Vec<Condition>,
    pub kind: Kind,
    pub combines_with: ClassSet,
    pub limits: Limits,
}

impl DiscountDraft {
    /// An active discount of `kind` for everyone, always, combining with nothing.
    pub fn new(title: &str, method: Method, kind: Kind) -> Self {
        Self {
            title: title.to_string(),
            method,
            status: Status::Active,
            starts_at: None,
            ends_at: None,
            conditions: Vec::new(),
            kind,
            combines_with: ClassSet::NONE,
            limits: Limits::default(),
        }
    }
}

/// A discount: automatic or by code, its classes derived from what it rewards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discount {
    id: DiscountId,
    version: Version,
    stamps: Option<Stamps>,
    title: String,
    method: Method,
    status: Status,
    starts_at: Option<Timestamp>,
    ends_at: Option<Timestamp>,
    classes: ClassSet,
    combines_with: ClassSet,
    conditions: Vec<Condition>,
    kind: Kind,
    limits: Limits,
    changes: Changes,
}

/// What happened since it was loaded, coalesced into one event per write. Compares as
/// always equal, so it never makes two discounts unequal.
#[derive(Debug, Clone, Default)]
struct Changes {
    created: bool,
    deleted: bool,
    fields: BTreeSet<&'static str>,
}

impl PartialEq for Changes {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for Changes {}

impl Recorded for Discount {
    fn take_events(&mut self) -> Vec<PendingEvent> {
        let changes = std::mem::take(&mut self.changes);
        let subject = Subject::new(DISCOUNT, self.id.as_uuid());
        if changes.deleted {
            return vec![PendingEvent::new(
                subject,
                &DiscountDeleted { discount: self.id },
            )];
        }
        if changes.created {
            return vec![PendingEvent::new(
                subject,
                &DiscountCreated { discount: self.id },
            )];
        }
        if changes.fields.is_empty() {
            return Vec::new();
        }
        let event = DiscountUpdated {
            discount: self.id,
            fields: changes.fields.into_iter().collect(),
        };
        vec![PendingEvent::new(subject, &event)]
    }
}

impl Versioned for Discount {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

impl Stamped for Discount {
    fn stamps(&self) -> Option<Stamps> {
        self.stamps
    }

    fn set_stamps(&mut self, stamps: Stamps) {
        self.stamps = Some(stamps);
    }
}

/// Plain-data form of a `Discount` for storage adapters. `restore` checks it like a new
/// one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscountSnapshot {
    pub id: DiscountId,
    pub version: Version,
    pub stamps: Option<Stamps>,
    pub title: String,
    pub method: Method,
    pub status: Status,
    pub starts_at: Option<Timestamp>,
    pub ends_at: Option<Timestamp>,
    pub conditions: Vec<Condition>,
    pub kind: Kind,
    pub combines_with: ClassSet,
    pub limits: Limits,
}

impl Discount {
    /// Checks every rule (`DiscountError::Invalid` lists each broken one) and records
    /// `discount.created`. Markets and currencies are checked apart
    /// (`check_currencies`), by a caller that can see markets.
    pub fn new(id: DiscountId, draft: DiscountDraft) -> Result<Self, DiscountError> {
        let (title, classes) = check(&draft)?;
        let mut discount = Self {
            id,
            version: Version::NEW,
            stamps: None,
            title,
            method: draft.method,
            status: draft.status,
            starts_at: draft.starts_at,
            ends_at: draft.ends_at,
            classes,
            combines_with: draft.combines_with,
            conditions: draft.conditions,
            kind: draft.kind,
            limits: draft.limits,
            changes: Changes::default(),
        };
        discount.changes.created = true;
        Ok(discount)
    }

    pub fn restore(s: DiscountSnapshot) -> Result<Self, DiscountError> {
        let draft = DiscountDraft {
            title: s.title,
            method: s.method,
            status: s.status,
            starts_at: s.starts_at,
            ends_at: s.ends_at,
            conditions: s.conditions,
            kind: s.kind,
            combines_with: s.combines_with,
            limits: s.limits,
        };
        let (title, classes) = check(&draft)?;
        Ok(Self {
            id: s.id,
            version: s.version,
            stamps: s.stamps,
            title,
            method: draft.method,
            status: draft.status,
            starts_at: draft.starts_at,
            ends_at: draft.ends_at,
            classes,
            combines_with: draft.combines_with,
            conditions: draft.conditions,
            kind: draft.kind,
            limits: draft.limits,
            changes: Changes::default(),
        })
    }

    pub fn snapshot(&self) -> DiscountSnapshot {
        DiscountSnapshot {
            id: self.id,
            version: self.version,
            stamps: self.stamps,
            title: self.title.clone(),
            method: self.method,
            status: self.status,
            starts_at: self.starts_at,
            ends_at: self.ends_at,
            conditions: self.conditions.clone(),
            kind: self.kind.clone(),
            combines_with: self.combines_with,
            limits: self.limits,
        }
    }

    /// The discount as a draft, to change and pass to `update`.
    pub fn draft(&self) -> DiscountDraft {
        DiscountDraft {
            title: self.title.clone(),
            method: self.method,
            status: self.status,
            starts_at: self.starts_at,
            ends_at: self.ends_at,
            conditions: self.conditions.clone(),
            kind: self.kind.clone(),
            combines_with: self.combines_with,
            limits: self.limits,
        }
    }

    /// Replaces everything with `draft`, checked like a new discount; what changed goes into
    /// `discount.updated`. The method is fixed (`discount_method_fixed`): a discount's codes
    /// would mean nothing to an automatic one.
    pub fn update(&mut self, draft: DiscountDraft) -> Result<(), DiscountError> {
        if draft.method != self.method {
            return Err(DiscountError::MethodFixed);
        }
        let (title, classes) = check(&draft)?;
        let mut fields = Vec::new();
        if title != self.title {
            fields.push("title");
        }
        if draft.status != self.status {
            fields.push("status");
        }
        if draft.starts_at != self.starts_at {
            fields.push("starts_at");
        }
        if draft.ends_at != self.ends_at {
            fields.push("ends_at");
        }
        if draft.conditions != self.conditions {
            fields.push("conditions");
        }
        if draft.kind != self.kind {
            fields.push("kind");
        }
        if draft.combines_with != self.combines_with {
            fields.push("combines_with");
        }
        if draft.limits != self.limits {
            fields.push("limits");
        }
        self.title = title;
        self.status = draft.status;
        self.starts_at = draft.starts_at;
        self.ends_at = draft.ends_at;
        self.classes = classes;
        self.conditions = draft.conditions;
        self.kind = draft.kind;
        self.combines_with = draft.combines_with;
        self.limits = draft.limits;
        self.changes.fields.extend(fields);
        Ok(())
    }

    /// Whether anything changed since it was loaded (or it's new or deleted).
    pub fn is_changed(&self) -> bool {
        self.changes.created || self.changes.deleted || !self.changes.fields.is_empty()
    }

    /// Records `discount.deleted`; the repository removes it.
    pub fn delete(&mut self) {
        self.changes.deleted = true;
    }

    /// Every amount must be for a market that exists (`currency_of` knows it) and in its
    /// currency.
    pub fn check_currencies(
        &self,
        currency_of: impl Fn(MarketId) -> Option<Currency>,
    ) -> Result<(), DiscountError> {
        let mut amounts = self.kind.amounts();
        for (i, condition) in self.conditions.iter().enumerate() {
            if let Condition::MinSubtotal(min) = condition {
                amounts.push((format!("conditions[{i}].amounts"), min));
            }
        }
        for (path, per_market) in amounts {
            for (market, amount) in per_market.iter() {
                let Some(expected) = currency_of(market) else {
                    return Err(DiscountError::MarketNotFound { path, market });
                };
                if amount.currency() != expected {
                    return Err(DiscountError::CurrencyMismatch {
                        path,
                        market,
                        expected,
                        found: amount.currency(),
                    });
                }
            }
        }
        Ok(())
    }

    /// Where it stands at `at`.
    pub fn phase(&self, at: Timestamp) -> Phase {
        if self.status == Status::Draft {
            return Phase::Draft;
        }
        if self.starts_at.is_some_and(|start| at < start) {
            return Phase::Scheduled;
        }
        if self.ends_at.is_some_and(|end| at >= end) {
            return Phase::Expired;
        }
        Phase::Active
    }

    /// Whether it may apply at `at`.
    pub fn is_live(&self, at: Timestamp) -> bool {
        self.phase(at) == Phase::Active
    }

    /// Whether, as an automatic discount, it counts toward `MAX_LIVE_AUTOMATIC` at `at`:
    /// active and not expired (a scheduled one will be live).
    pub fn counts_as_live_automatic(&self, at: Timestamp) -> bool {
        self.method == Method::Automatic
            && matches!(self.phase(at), Phase::Active | Phase::Scheduled)
    }

    /// Whether `self` and `other` may apply together: each combines with every class of
    /// the other.
    pub fn combines(&self, other: &Discount) -> bool {
        self.combines_with.covers(other.classes) && other.combines_with.covers(self.classes)
    }

    pub fn id(&self) -> DiscountId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn method(&self) -> Method {
        self.method
    }

    pub fn status(&self) -> Status {
        self.status
    }

    pub fn starts_at(&self) -> Option<Timestamp> {
        self.starts_at
    }

    pub fn ends_at(&self) -> Option<Timestamp> {
        self.ends_at
    }

    pub fn classes(&self) -> ClassSet {
        self.classes
    }

    pub fn combines_with(&self) -> ClassSet {
        self.combines_with
    }

    pub fn conditions(&self) -> &[Condition] {
        &self.conditions
    }

    pub fn kind(&self) -> &Kind {
        &self.kind
    }

    pub fn limits(&self) -> Limits {
        self.limits
    }
}

/// Every rule of a draft; the trimmed title and the classes when they all hold.
fn check(draft: &DiscountDraft) -> Result<(String, ClassSet), DiscountError> {
    let mut p = Problems::default();
    let title = match required_text("title", &draft.title) {
        Ok(title) => title,
        Err(e) => {
            p.add("title", "invalid_text", e.to_string());
            String::new()
        }
    };
    if let (Some(start), Some(end)) = (draft.starts_at, draft.ends_at)
        && end <= start
    {
        p.add(
            "ends_at",
            "ends_not_after_start",
            "ends at or before it starts",
        );
    }
    if draft.limits.total == Some(0) {
        p.add(
            "limits.total",
            "too_small",
            "at least 1, or null for no limit",
        );
    }
    check_conditions(&draft.conditions, &mut p);
    check_kind(&draft.kind, &mut p);
    p.into_result().map(|()| (title, draft.kind.classes()))
}

#[derive(Default)]
struct Problems(Vec<Problem>);

impl Problems {
    fn add(&mut self, path: impl Into<String>, code: &'static str, message: impl Into<String>) {
        self.0.push(Problem {
            path: path.into(),
            code,
            message: message.into(),
        });
    }

    fn into_result(self) -> Result<(), DiscountError> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(DiscountError::Invalid(self.0))
        }
    }
}

/// A list of IDs: at least one, at most `MAX_IDS`, each once.
fn check_ids<T: Eq + std::hash::Hash>(ids: &[T], path: &str, p: &mut Problems) {
    if ids.is_empty() {
        p.add(path, "empty", "list at least one");
    } else if ids.len() > MAX_IDS {
        p.add(path, "too_many", format!("at most {MAX_IDS}"));
    } else {
        let mut seen = HashSet::with_capacity(ids.len());
        if let Some(i) = ids.iter().position(|id| !seen.insert(id)) {
            p.add(format!("{path}[{i}]"), "duplicate", "listed twice");
        }
    }
}

/// Amounts per market: at least one, each above zero (or at zero when `zero_ok`).
fn check_amounts(amounts: &PerMarket, path: &str, zero_ok: bool, p: &mut Problems) {
    if amounts.is_empty() {
        p.add(path, "empty", "give an amount for at least one market");
    }
    for (market, amount) in amounts.iter() {
        if amount.amount() < 0 || (!zero_ok && amount.amount() == 0) {
            let least = if zero_ok { "zero" } else { "above zero" };
            p.add(
                format!("{path}.{market}"),
                "too_small",
                format!("at least {least}"),
            );
        }
    }
}

fn check_conditions(conditions: &[Condition], p: &mut Problems) {
    let mut seen = HashSet::new();
    for (i, condition) in conditions.iter().enumerate() {
        let path = format!("conditions[{i}]");
        if !seen.insert(condition.name()) {
            p.add(&path, "duplicate", "each condition at most once");
        }
        match condition {
            Condition::Markets(ids) => check_ids(ids, &format!("{path}.markets"), p),
            Condition::Channels(ids) => check_ids(ids, &format!("{path}.channels"), p),
            Condition::CustomerGroups(ids) => check_ids(ids, &format!("{path}.groups"), p),
            Condition::Customers(ids) => check_ids(ids, &format!("{path}.customers"), p),
            Condition::SignedIn => {}
            Condition::MinSubtotal(amounts) => {
                check_amounts(amounts, &format!("{path}.amounts"), false, p);
            }
            Condition::MinQuantity(n) => {
                if *n == 0 {
                    p.add(format!("{path}.quantity"), "too_small", "at least 1");
                }
            }
        }
    }
}

fn check_items(set: &ItemSet, path: &str, p: &mut Problems) {
    match &set.items {
        Items::All => {}
        Items::Products(ids) => check_ids(ids, &format!("{path}.ids"), p),
        Items::Variants(ids) => check_ids(ids, &format!("{path}.ids"), p),
        Items::Categories(ids) => check_ids(ids, &format!("{path}.ids"), p),
    }
}

fn check_percent(bp: BasisPoints, path: &str, p: &mut Problems) {
    if !(1..=10_000).contains(&bp.value()) {
        p.add(path, "out_of_range", "1 to 10000 basis points");
    }
}

/// A value for `target`'s class: anything for items; a percentage or an amount across for
/// the subtotal and shipping.
fn check_value(value: &Value, target: Class, path: &str, p: &mut Problems) {
    match value {
        Value::Percent(bp) => check_percent(*bp, &format!("{path}.basis_points"), p),
        Value::Amount {
            amounts,
            allocation,
        } => {
            check_amounts(amounts, &format!("{path}.amounts"), false, p);
            match allocation {
                Allocation::Across => {}
                Allocation::Each | Allocation::Cheapest { .. } if target != Class::Product => {
                    p.add(
                        format!("{path}.allocation"),
                        "allocation_not_allowed",
                        format!("the {} takes an amount across", target.as_str()),
                    );
                }
                Allocation::Cheapest { units: 0 } => {
                    p.add(
                        format!("{path}.allocation.units"),
                        "too_small",
                        "at least 1",
                    );
                }
                Allocation::Each | Allocation::Cheapest { .. } => {}
            }
        }
        Value::FixedPrice(amounts) => {
            if target != Class::Product {
                p.add(
                    format!("{path}.type"),
                    "value_not_allowed",
                    "a fixed price is for items only",
                );
            }
            check_amounts(amounts, &format!("{path}.amounts"), true, p);
        }
    }
}

fn check_kind(kind: &Kind, p: &mut Problems) {
    match kind {
        Kind::Standard { rewards } => {
            if rewards.is_empty() || rewards.len() > MAX_REWARDS {
                p.add(
                    "kind.rewards",
                    "out_of_range",
                    format!("1 to {MAX_REWARDS} rewards"),
                );
            }
            let mut classes = HashSet::new();
            for (i, reward) in rewards.iter().enumerate() {
                let path = format!("kind.rewards[{i}]");
                let class = reward.target.class();
                if !classes.insert(class) {
                    p.add(
                        format!("{path}.target"),
                        "duplicate_class",
                        format!("one reward per class; {} is taken", class.as_str()),
                    );
                }
                match &reward.target {
                    Target::Items(set) => check_items(set, &format!("{path}.target.items"), p),
                    Target::Subtotal => {}
                    Target::Shipping { max_amount } => {
                        if let Some(max) = max_amount {
                            check_amounts(max, &format!("{path}.target.max_amount"), false, p);
                        }
                    }
                }
                check_value(&reward.value, class, &format!("{path}.value"), p);
            }
        }
        Kind::BuyXGetY {
            buys,
            buy,
            gets,
            get_quantity,
            value,
            uses_per_order,
        } => {
            check_items(buys, "kind.buys.items", p);
            check_items(gets, "kind.gets.items", p);
            match buy {
                Threshold::Quantity(0) => p.add("kind.buy.quantity", "too_small", "at least 1"),
                Threshold::Quantity(_) => {}
                Threshold::Amount(amounts) => check_amounts(amounts, "kind.buy.amounts", false, p),
            }
            if *get_quantity == 0 {
                p.add("kind.get_quantity", "too_small", "at least 1");
            }
            match value {
                Value::Percent(bp) => check_percent(*bp, "kind.value.basis_points", p),
                Value::Amount {
                    amounts,
                    allocation: Allocation::Each,
                } => check_amounts(amounts, "kind.value.amounts", false, p),
                Value::Amount { .. } => p.add(
                    "kind.value.allocation",
                    "allocation_not_allowed",
                    "an amount off each unit got",
                ),
                Value::FixedPrice(_) => p.add(
                    "kind.value.type",
                    "value_not_allowed",
                    "a percentage or an amount off each unit",
                ),
            }
            if *uses_per_order == Some(0) {
                p.add("kind.uses_per_order", "too_small", "at least 1, or null");
            }
        }
        Kind::Tiered {
            items,
            measure,
            tiers,
        } => {
            check_items(items, "kind.items.items", p);
            if tiers.is_empty() || tiers.len() > MAX_TIERS {
                p.add(
                    "kind.tiers",
                    "out_of_range",
                    format!("1 to {MAX_TIERS} tiers"),
                );
            }
            for (i, tier) in tiers.iter().enumerate() {
                let path = format!("kind.tiers[{i}]");
                match (&tier.threshold, measure) {
                    (Threshold::Quantity(0), Measure::Quantity) => p.add(
                        format!("{path}.threshold.quantity"),
                        "too_small",
                        "at least 1",
                    ),
                    (Threshold::Quantity(_), Measure::Quantity) => {}
                    (Threshold::Amount(amounts), Measure::Subtotal) => {
                        check_amounts(amounts, &format!("{path}.threshold.amounts"), false, p);
                    }
                    _ => p.add(
                        format!("{path}.threshold"),
                        "threshold_mismatch",
                        format!("a {} threshold", measure.as_str()),
                    ),
                }
                match &tier.value {
                    Value::Percent(bp) => {
                        check_percent(*bp, &format!("{path}.value.basis_points"), p);
                    }
                    Value::Amount {
                        amounts,
                        allocation: Allocation::Across,
                    } => check_amounts(amounts, &format!("{path}.value.amounts"), false, p),
                    Value::Amount { .. } => p.add(
                        format!("{path}.value.allocation"),
                        "allocation_not_allowed",
                        "a tier takes an amount across its items",
                    ),
                    Value::FixedPrice(_) => p.add(
                        format!("{path}.value.type"),
                        "value_not_allowed",
                        "a percentage or an amount",
                    ),
                }
                if i > 0 {
                    check_ascending(&tiers[i - 1].threshold, &tier.threshold, &path, p);
                }
            }
        }
    }
}

/// Tiers rise: each threshold above the one before, an amount in every market the first
/// tier names and no other.
fn check_ascending(before: &Threshold, now: &Threshold, path: &str, p: &mut Problems) {
    match (before, now) {
        (Threshold::Quantity(a), Threshold::Quantity(b)) => {
            if b <= a {
                p.add(
                    format!("{path}.threshold"),
                    "not_ascending",
                    "above the tier before",
                );
            }
        }
        (Threshold::Amount(a), Threshold::Amount(b)) => {
            let markets_a: Vec<MarketId> = a.iter().map(|(m, _)| m).collect();
            let markets_b: Vec<MarketId> = b.iter().map(|(m, _)| m).collect();
            if markets_a != markets_b {
                p.add(
                    format!("{path}.threshold.amounts"),
                    "markets_differ",
                    "the same markets as the tier before",
                );
                return;
            }
            for ((market, x), (_, y)) in a.iter().zip(b.iter()) {
                if y.currency() == x.currency() && y.amount() <= x.amount() {
                    p.add(
                        format!("{path}.threshold.amounts.{market}"),
                        "not_ascending",
                        "above the tier before",
                    );
                }
            }
        }
        // Reported as a threshold mismatch already.
        _ => {}
    }
}

#[cfg(test)]
mod tests;
