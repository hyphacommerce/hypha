//! Discounts and their codes (`discounts:*`). A discount's rules are typed JSON: its
//! `conditions` (all must hold), its `kind` (standard rewards, buy X get Y, or tiers), the
//! classes it `combines_with` and its `limits`. Amounts are per market, each in the
//! market's currency.

use std::collections::{BTreeMap, HashMap};

use axum::{
    Json,
    body::Body,
    extract::State,
    http::{StatusCode, header},
    response::Response,
};
use hypha_core::{
    BasisPoints, CategoryId, ChannelId, Clock, Currency, CustomerGroupId, CustomerId, DiscountId,
    MarketId, Money, OrderId, PageLimit, PageRequest, ProductId, Stamped, VariantId,
};
use hypha_customers::{CustomerFilter, CustomerRepo, CustomerSort, CustomerSummary, HasCustomers};
use hypha_discounts::{
    Allocation, Audience, Class, ClassSet, CodeFilter, CodeSelection, Condition, CreateDiscount,
    Discount, DiscountCode, DiscountDraft, DiscountFilter, DiscountPatch, DiscountRepo, Generation,
    ItemSet, Items, Kind, Limits, Measure, Method, NameRequest, Names, NewCode, PerMarket, Phase,
    Reward, Status, SummaryDraft, Target, Threshold, Tier, Usage, Value, add_codes,
    create_discount, delete_discount, generate_codes, get_discount, list_codes, list_discounts,
    list_usage, remove_codes, set_codes_active, summarize, summary, update_discount,
};
use hypha_markets::{HasMarkets, MarketRepo};
use hypha_orders::{HasOrders, OrderNumber, OrderRepo};
use serde::{Deserialize, Serialize};

use super::{
    markets::Instant,
    page::{ListQuery, PageParams, PageResponse},
    rfc3339, stamp_times,
};
use crate::{
    error::ApiError,
    extract::{Caller, JsonBody, PathId},
    patch,
    redact::{self, Family, Redacted},
    state::{SharedState, Storage},
};

// ---------- the rules ----------

/// Amounts per market (keys are market IDs), each in that market's currency.
pub type DiscountAmountsDto = BTreeMap<MarketId, Money>;

fn amounts_in(a: DiscountAmountsDto) -> PerMarket {
    PerMarket::new(a)
}

fn amounts_out(p: &PerMarket) -> DiscountAmountsDto {
    p.iter().collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DiscountMethodDto {
    /// Applies by itself whenever its conditions hold.
    Automatic,
    /// Applies only to carts holding one of its codes.
    Codes,
}

impl From<DiscountMethodDto> for Method {
    fn from(m: DiscountMethodDto) -> Self {
        match m {
            DiscountMethodDto::Automatic => Method::Automatic,
            DiscountMethodDto::Codes => Method::Codes,
        }
    }
}

impl From<Method> for DiscountMethodDto {
    fn from(m: Method) -> Self {
        match m {
            Method::Automatic => DiscountMethodDto::Automatic,
            Method::Codes => DiscountMethodDto::Codes,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DiscountStatusDto {
    /// Never applies, whatever its dates.
    Draft,
    /// Applies between its dates.
    Active,
}

impl From<DiscountStatusDto> for Status {
    fn from(s: DiscountStatusDto) -> Self {
        match s {
            DiscountStatusDto::Draft => Status::Draft,
            DiscountStatusDto::Active => Status::Active,
        }
    }
}

impl From<Status> for DiscountStatusDto {
    fn from(s: Status) -> Self {
        match s {
            Status::Draft => DiscountStatusDto::Draft,
            Status::Active => DiscountStatusDto::Active,
        }
    }
}

/// Where a discount stands now, from its status and dates.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DiscountPhaseDto {
    Draft,
    Scheduled,
    Active,
    Expired,
}

impl From<Phase> for DiscountPhaseDto {
    fn from(p: Phase) -> Self {
        match p {
            Phase::Draft => DiscountPhaseDto::Draft,
            Phase::Scheduled => DiscountPhaseDto::Scheduled,
            Phase::Active => DiscountPhaseDto::Active,
            Phase::Expired => DiscountPhaseDto::Expired,
        }
    }
}

impl From<DiscountPhaseDto> for Phase {
    fn from(p: DiscountPhaseDto) -> Self {
        match p {
            DiscountPhaseDto::Draft => Phase::Draft,
            DiscountPhaseDto::Scheduled => Phase::Scheduled,
            DiscountPhaseDto::Active => Phase::Active,
            DiscountPhaseDto::Expired => Phase::Expired,
        }
    }
}

/// What a reward reduces, for combining: `product` (item lines), `order` (the subtotal
/// after product discounts), `shipping`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DiscountClassDto {
    Product,
    Order,
    Shipping,
}

impl From<DiscountClassDto> for Class {
    fn from(c: DiscountClassDto) -> Self {
        match c {
            DiscountClassDto::Product => Class::Product,
            DiscountClassDto::Order => Class::Order,
            DiscountClassDto::Shipping => Class::Shipping,
        }
    }
}

impl From<Class> for DiscountClassDto {
    fn from(c: Class) -> Self {
        match c {
            Class::Product => DiscountClassDto::Product,
            Class::Order => DiscountClassDto::Order,
            Class::Shipping => DiscountClassDto::Shipping,
        }
    }
}

fn classes_in(classes: Vec<DiscountClassDto>) -> ClassSet {
    let classes: Vec<Class> = classes.into_iter().map(Into::into).collect();
    ClassSet::of(&classes)
}

fn classes_out(set: ClassSet) -> Vec<DiscountClassDto> {
    set.classes().into_iter().map(Into::into).collect()
}

/// A condition: all of a discount's must hold; inside one, any of its values will do.
/// Lists hold 1 to 1,000 IDs, each once; each condition type appears at most once.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum DiscountConditionDto {
    /// The cart's market is one of these.
    Markets { markets: Vec<MarketId> },
    /// The cart's sales channel is one of these.
    Channels { channels: Vec<ChannelId> },
    /// The buyer is in any of these groups.
    CustomerGroups { groups: Vec<CustomerGroupId> },
    /// The buyer is one of these customers (a guest by their email's customer).
    Customers { customers: Vec<CustomerId> },
    /// The cart belongs to a customer: signed in, or linked by staff.
    SignedIn,
    /// The qualifying items cost at least this much in the cart's market: before discounts
    /// for a discount that reduces items, after product discounts otherwise. A market
    /// without an amount isn't served.
    MinSubtotal {
        #[schema(value_type = std::collections::HashMap<String, Money>)]
        amounts: DiscountAmountsDto,
    },
    /// The cart holds at least this many qualifying units.
    MinQuantity { quantity: u32 },
}

impl From<DiscountConditionDto> for Condition {
    fn from(c: DiscountConditionDto) -> Self {
        match c {
            DiscountConditionDto::Markets { markets } => Condition::Markets(markets),
            DiscountConditionDto::Channels { channels } => Condition::Channels(channels),
            DiscountConditionDto::CustomerGroups { groups } => Condition::CustomerGroups(groups),
            DiscountConditionDto::Customers { customers } => Condition::Customers(customers),
            DiscountConditionDto::SignedIn => Condition::SignedIn,
            DiscountConditionDto::MinSubtotal { amounts } => {
                Condition::MinSubtotal(amounts_in(amounts))
            }
            DiscountConditionDto::MinQuantity { quantity } => Condition::MinQuantity(quantity),
        }
    }
}

impl From<&Condition> for DiscountConditionDto {
    fn from(c: &Condition) -> Self {
        match c {
            Condition::Markets(ids) => DiscountConditionDto::Markets {
                markets: ids.clone(),
            },
            Condition::Channels(ids) => DiscountConditionDto::Channels {
                channels: ids.clone(),
            },
            Condition::CustomerGroups(ids) => DiscountConditionDto::CustomerGroups {
                groups: ids.clone(),
            },
            Condition::Customers(ids) => DiscountConditionDto::Customers {
                customers: ids.clone(),
            },
            Condition::SignedIn => DiscountConditionDto::SignedIn,
            Condition::MinSubtotal(a) => DiscountConditionDto::MinSubtotal {
                amounts: amounts_out(a),
            },
            Condition::MinQuantity(n) => DiscountConditionDto::MinQuantity { quantity: *n },
        }
    }
}

/// Which items: all, or listed products, variants or categories (a category reaches the
/// products in it and in every category below it).
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum DiscountItemsDto {
    All,
    Products {
        #[schema(value_type = Vec<ProductId>)]
        ids: Vec<ProductId>,
    },
    Variants {
        #[schema(value_type = Vec<VariantId>)]
        ids: Vec<VariantId>,
    },
    Categories {
        #[schema(value_type = Vec<CategoryId>)]
        ids: Vec<CategoryId>,
    },
}

impl From<DiscountItemsDto> for Items {
    fn from(i: DiscountItemsDto) -> Self {
        match i {
            DiscountItemsDto::All => Items::All,
            DiscountItemsDto::Products { ids } => Items::Products(ids),
            DiscountItemsDto::Variants { ids } => Items::Variants(ids),
            DiscountItemsDto::Categories { ids } => Items::Categories(ids),
        }
    }
}

impl From<&Items> for DiscountItemsDto {
    fn from(i: &Items) -> Self {
        match i {
            Items::All => DiscountItemsDto::All,
            Items::Products(ids) => DiscountItemsDto::Products { ids: ids.clone() },
            Items::Variants(ids) => DiscountItemsDto::Variants { ids: ids.clone() },
            Items::Categories(ids) => DiscountItemsDto::Categories { ids: ids.clone() },
        }
    }
}

/// A set of items, optionally leaving out those on sale (shown against a compare-at
/// price).
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DiscountItemSetDto {
    items: DiscountItemsDto,
    /// Defaults to `false`.
    #[serde(default)]
    exclude_on_sale: bool,
}

impl From<DiscountItemSetDto> for ItemSet {
    fn from(s: DiscountItemSetDto) -> Self {
        ItemSet {
            items: s.items.into(),
            exclude_on_sale: s.exclude_on_sale,
        }
    }
}

impl From<&ItemSet> for DiscountItemSetDto {
    fn from(s: &ItemSet) -> Self {
        Self {
            items: (&s.items).into(),
            exclude_on_sale: s.exclude_on_sale,
        }
    }
}

/// How an amount off is spread over the items it reaches.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum DiscountAllocationDto {
    /// Off every unit (items only).
    Each,
    /// Once, split over the items (or the order, or shipping) in proportion.
    Across,
    /// Off each of the cheapest `units` units (items only).
    Cheapest { units: u32 },
}

impl From<DiscountAllocationDto> for Allocation {
    fn from(a: DiscountAllocationDto) -> Self {
        match a {
            DiscountAllocationDto::Each => Allocation::Each,
            DiscountAllocationDto::Across => Allocation::Across,
            DiscountAllocationDto::Cheapest { units } => Allocation::Cheapest { units },
        }
    }
}

impl From<Allocation> for DiscountAllocationDto {
    fn from(a: Allocation) -> Self {
        match a {
            Allocation::Each => DiscountAllocationDto::Each,
            Allocation::Across => DiscountAllocationDto::Across,
            Allocation::Cheapest { units } => DiscountAllocationDto::Cheapest { units },
        }
    }
}

fn across() -> DiscountAllocationDto {
    DiscountAllocationDto::Across
}

/// How much comes off.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum DiscountValueDto {
    /// 1 to 10000 basis points: 1000 is 10%, 10000 everything (free).
    Percent { basis_points: i32 },
    /// An amount per market; `allocation` defaults to `across`.
    Amount {
        #[schema(value_type = std::collections::HashMap<String, Money>)]
        amounts: DiscountAmountsDto,
        #[serde(default = "across")]
        allocation: DiscountAllocationDto,
    },
    /// Items only: each unit costs this much in the market, when that's lower than its
    /// price (0 makes them free).
    FixedPrice {
        #[schema(value_type = std::collections::HashMap<String, Money>)]
        amounts: DiscountAmountsDto,
    },
}

impl DiscountValueDto {
    fn into_value(self) -> Result<Value, ApiError> {
        Ok(match self {
            DiscountValueDto::Percent { basis_points } => Value::Percent(
                // Below -10000 can't be a basis point at all: reported like any other range.
                BasisPoints::new(basis_points.max(0)).map_err(ApiError::internal)?,
            ),
            DiscountValueDto::Amount {
                amounts,
                allocation,
            } => Value::Amount {
                amounts: amounts_in(amounts),
                allocation: allocation.into(),
            },
            DiscountValueDto::FixedPrice { amounts } => Value::FixedPrice(amounts_in(amounts)),
        })
    }
}

impl From<&Value> for DiscountValueDto {
    fn from(v: &Value) -> Self {
        match v {
            Value::Percent(bp) => DiscountValueDto::Percent {
                basis_points: bp.value(),
            },
            Value::Amount {
                amounts,
                allocation,
            } => DiscountValueDto::Amount {
                amounts: amounts_out(amounts),
                allocation: (*allocation).into(),
            },
            Value::FixedPrice(amounts) => DiscountValueDto::FixedPrice {
                amounts: amounts_out(amounts),
            },
        }
    }
}

/// What a reward reduces: items (its class is `product`), the subtotal (`order`), or
/// shipping (`shipping`, at most `max_amount` off when given).
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum DiscountTargetDto {
    Items {
        items: DiscountItemsDto,
        /// Defaults to `false`.
        #[serde(default)]
        exclude_on_sale: bool,
    },
    Subtotal,
    Shipping {
        #[serde(default)]
        #[schema(value_type = Option<std::collections::HashMap<String, Money>>)]
        max_amount: Option<DiscountAmountsDto>,
    },
}

impl From<DiscountTargetDto> for Target {
    fn from(t: DiscountTargetDto) -> Self {
        match t {
            DiscountTargetDto::Items {
                items,
                exclude_on_sale,
            } => Target::Items(ItemSet {
                items: items.into(),
                exclude_on_sale,
            }),
            DiscountTargetDto::Subtotal => Target::Subtotal,
            DiscountTargetDto::Shipping { max_amount } => Target::Shipping {
                max_amount: max_amount.map(amounts_in),
            },
        }
    }
}

impl From<&Target> for DiscountTargetDto {
    fn from(t: &Target) -> Self {
        match t {
            Target::Items(set) => DiscountTargetDto::Items {
                items: (&set.items).into(),
                exclude_on_sale: set.exclude_on_sale,
            },
            Target::Subtotal => DiscountTargetDto::Subtotal,
            Target::Shipping { max_amount } => DiscountTargetDto::Shipping {
                max_amount: max_amount.as_ref().map(amounts_out),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DiscountRewardDto {
    target: DiscountTargetDto,
    value: DiscountValueDto,
}

/// What must be bought (buy X get Y) or reached (a tier).
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum DiscountThresholdDto {
    Quantity {
        quantity: u32,
    },
    /// Per market, at prices before discounts.
    Amount {
        #[schema(value_type = std::collections::HashMap<String, Money>)]
        amounts: DiscountAmountsDto,
    },
}

impl From<DiscountThresholdDto> for Threshold {
    fn from(t: DiscountThresholdDto) -> Self {
        match t {
            DiscountThresholdDto::Quantity { quantity } => Threshold::Quantity(quantity),
            DiscountThresholdDto::Amount { amounts } => Threshold::Amount(amounts_in(amounts)),
        }
    }
}

impl From<&Threshold> for DiscountThresholdDto {
    fn from(t: &Threshold) -> Self {
        match t {
            Threshold::Quantity(n) => DiscountThresholdDto::Quantity { quantity: *n },
            Threshold::Amount(a) => DiscountThresholdDto::Amount {
                amounts: amounts_out(a),
            },
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DiscountMeasureDto {
    /// Units of the items.
    Quantity,
    /// What the items cost before discounts.
    Subtotal,
}

/// From `threshold` (a quantity or an amount, as the tiers' `measure` says), `value` off
/// the items: a percentage, or an amount spread across them.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DiscountTierDto {
    threshold: DiscountThresholdDto,
    value: DiscountValueDto,
}

/// The shape of a discount. Its classes follow: a standard discount's are its rewards'
/// targets', buy X get Y and tiers are `product`.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum DiscountKindDto {
    /// One to three rewards, at most one per class ("20% off cultures + free shipping").
    Standard { rewards: Vec<DiscountRewardDto> },
    /// Buy `buy` of `buys` (at full price, not reduced by another discount), get
    /// `get_quantity` of the cheapest units of `gets` at `value` off; repeats while the cart
    /// qualifies, at most `uses_per_order` times (`null`: no limit).
    BuyXGetY {
        buys: DiscountItemSetDto,
        buy: DiscountThresholdDto,
        gets: DiscountItemSetDto,
        get_quantity: u32,
        /// A percentage (10000 is free) or an amount off each unit got (`allocation:
        /// each`).
        value: DiscountValueDto,
        #[serde(default)]
        uses_per_order: Option<u32>,
    },
    /// The highest of up to 10 rising tiers the items reach applies to them.
    Tiered {
        items: DiscountItemSetDto,
        measure: DiscountMeasureDto,
        tiers: Vec<DiscountTierDto>,
    },
}

impl DiscountKindDto {
    fn into_kind(self) -> Result<Kind, ApiError> {
        Ok(match self {
            DiscountKindDto::Standard { rewards } => Kind::Standard {
                rewards: rewards
                    .into_iter()
                    .map(|r| {
                        Ok(Reward {
                            target: r.target.into(),
                            value: r.value.into_value()?,
                        })
                    })
                    .collect::<Result<_, ApiError>>()?,
            },
            DiscountKindDto::BuyXGetY {
                buys,
                buy,
                gets,
                get_quantity,
                value,
                uses_per_order,
            } => Kind::BuyXGetY {
                buys: buys.into(),
                buy: buy.into(),
                gets: gets.into(),
                get_quantity,
                value: value.into_value()?,
                uses_per_order,
            },
            DiscountKindDto::Tiered {
                items,
                measure,
                tiers,
            } => Kind::Tiered {
                items: items.into(),
                measure: match measure {
                    DiscountMeasureDto::Quantity => Measure::Quantity,
                    DiscountMeasureDto::Subtotal => Measure::Subtotal,
                },
                tiers: tiers
                    .into_iter()
                    .map(|t| {
                        Ok(Tier {
                            threshold: t.threshold.into(),
                            value: t.value.into_value()?,
                        })
                    })
                    .collect::<Result<_, ApiError>>()?,
            },
        })
    }
}

impl From<&Kind> for DiscountKindDto {
    fn from(k: &Kind) -> Self {
        match k {
            Kind::Standard { rewards } => DiscountKindDto::Standard {
                rewards: rewards
                    .iter()
                    .map(|r| DiscountRewardDto {
                        target: (&r.target).into(),
                        value: (&r.value).into(),
                    })
                    .collect(),
            },
            Kind::BuyXGetY {
                buys,
                buy,
                gets,
                get_quantity,
                value,
                uses_per_order,
            } => DiscountKindDto::BuyXGetY {
                buys: buys.into(),
                buy: buy.into(),
                gets: gets.into(),
                get_quantity: *get_quantity,
                value: value.into(),
                uses_per_order: *uses_per_order,
            },
            Kind::Tiered {
                items,
                measure,
                tiers,
            } => DiscountKindDto::Tiered {
                items: items.into(),
                measure: match measure {
                    Measure::Quantity => DiscountMeasureDto::Quantity,
                    Measure::Subtotal => DiscountMeasureDto::Subtotal,
                },
                tiers: tiers
                    .iter()
                    .map(|t| DiscountTierDto {
                        threshold: (&t.threshold).into(),
                        value: (&t.value).into(),
                    })
                    .collect(),
            },
        }
    }
}

/// How often a discount may be used. Uses are counted when orders are placed and not
/// given back when an order is cancelled.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DiscountLimitsDto {
    /// Orders it may apply to in all; `null` for no limit.
    #[serde(default)]
    total: Option<u32>,
    /// At most one order per customer (a guest counts by their email's customer).
    #[serde(default)]
    once_per_customer: bool,
}

impl From<DiscountLimitsDto> for Limits {
    fn from(l: DiscountLimitsDto) -> Self {
        Limits {
            total: l.total,
            once_per_customer: l.once_per_customer,
        }
    }
}

impl From<Limits> for DiscountLimitsDto {
    fn from(l: Limits) -> Self {
        Self {
            total: l.total,
            once_per_customer: l.once_per_customer,
        }
    }
}

// ---------- discounts ----------

#[derive(Serialize, utoipa::ToSchema)]
pub struct DiscountResponse {
    id: DiscountId,
    /// Shown to buyers.
    title: String,
    method: DiscountMethodDto,
    status: DiscountStatusDto,
    /// Where it stands now, from `status` and the dates.
    phase: DiscountPhaseDto,
    /// RFC 3339; `null` for at once.
    starts_at: Option<String>,
    /// RFC 3339, exclusive; `null` for never.
    ends_at: Option<String>,
    /// What its rewards reduce.
    classes: Vec<DiscountClassDto>,
    /// The classes of the discounts it may apply together with. Two apply together only
    /// when each allows every class of the other.
    combines_with: Vec<DiscountClassDto>,
    conditions: Vec<DiscountConditionDto>,
    kind: DiscountKindDto,
    limits: DiscountLimitsDto,
    /// One sentence from its data, naming what it applies to ("10% off Grow kits on
    /// orders from €50"): what the admin, the storefront and agents show alike. Customers
    /// are named only to a caller who may see their names; otherwise counted.
    summary: String,
    /// How many codes it has (0 for an automatic one).
    #[schema(minimum = 0)]
    code_count: u64,
    /// Orders it was used on.
    #[schema(minimum = 0)]
    used: u64,
    /// What it took off those orders, one sum per currency, by currency code; `[]` while
    /// unused.
    discounted_total: Vec<Money>,
    created_at: String,
    updated_at: String,
}

impl DiscountResponse {
    fn new(
        d: &Discount,
        now: hypha_core::Timestamp,
        code_count: u64,
        used: u64,
        discounted_total: Vec<Money>,
        names: &Names,
    ) -> Result<Self, ApiError> {
        let (created_at, updated_at) = stamp_times(d.stamps())?;
        Ok(Self {
            id: d.id(),
            title: d.title().to_string(),
            method: d.method().into(),
            status: d.status().into(),
            phase: d.phase(now).into(),
            starts_at: d.starts_at().map(rfc3339).transpose()?,
            ends_at: d.ends_at().map(rfc3339).transpose()?,
            classes: classes_out(d.classes()),
            combines_with: classes_out(d.combines_with()),
            conditions: d.conditions().iter().map(Into::into).collect(),
            kind: d.kind().into(),
            limits: d.limits().into(),
            summary: summary(d, None, names, Audience::Staff),
            code_count,
            used,
            discounted_total,
            created_at,
            updated_at,
        })
    }
}

/// What staff summaries of `discounts` name, in one read. Customers are named only to a
/// caller who may read customers and see their names.
async fn staff_names<P: hypha_discounts::HasDiscounts>(
    ports: &P,
    ctx: &hypha_core::Ctx,
    discounts: &[Discount],
) -> Result<Names, ApiError> {
    let mut request = NameRequest::for_staff(discounts);
    if !ctx.has_scope(hypha_core::Scope::CustomersRead)
        || crate::redact::Redaction::of(ctx).removes_name()
    {
        request.customers.clear();
    }
    if request.is_empty() {
        return Ok(Names::new());
    }
    Ok(ports
        .discounts()
        .names(&request)
        .await
        .map_err(hypha_discounts::CommandError::from)?)
}

/// `discounts` as responses, with their code counts, uses and names read once for them
/// all.
async fn responses<P: hypha_discounts::HasDiscounts>(
    ports: &P,
    ctx: &hypha_core::Ctx,
    discounts: &[Discount],
    now: hypha_core::Timestamp,
) -> Result<Vec<DiscountResponse>, ApiError> {
    let names = staff_names(ports, ctx, discounts).await?;
    let ids: Vec<DiscountId> = discounts.iter().map(Discount::id).collect();
    let repo = ports.discounts();
    let codes = repo
        .code_counts(&ids)
        .await
        .map_err(hypha_discounts::CommandError::from)?;
    let used = repo
        .usage_counts(&ids)
        .await
        .map_err(hypha_discounts::CommandError::from)?;
    let mut totals = repo
        .discounted_totals(&ids)
        .await
        .map_err(hypha_discounts::CommandError::from)?;
    discounts
        .iter()
        .map(|d| {
            DiscountResponse::new(
                d,
                now,
                codes.get(&d.id()).copied().unwrap_or(0),
                used.get(&d.id()).copied().unwrap_or(0),
                totals.remove(&d.id()).unwrap_or_default(),
                &names,
            )
        })
        .collect()
}

async fn one<P: hypha_discounts::HasDiscounts>(
    ports: &P,
    ctx: &hypha_core::Ctx,
    d: &Discount,
    now: hypha_core::Timestamp,
) -> Result<DiscountResponse, ApiError> {
    responses(ports, ctx, std::slice::from_ref(d), now)
        .await?
        .pop()
        .ok_or_else(|| ApiError::internal("a discount without a response"))
}

/// Each market's currency: what a discount's amounts are checked against.
async fn currencies<P: HasMarkets>(ports: &P) -> Result<HashMap<MarketId, Currency>, ApiError> {
    Ok(ports
        .markets()
        .list_all()
        .await
        .map_err(hypha_discounts::CommandError::from)?
        .iter()
        .map(|m| (m.id(), m.currency()))
        .collect())
}

#[derive(Deserialize, utoipa::IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct DiscountFilters {
    /// Where they stand now.
    #[param(inline)]
    status: Option<DiscountPhaseDto>,
    #[param(inline)]
    method: Option<DiscountMethodDto>,
    /// Those with a reward of this class.
    #[param(inline)]
    class: Option<DiscountClassDto>,
    /// In the title, ignoring case, or a part of one of its codes.
    q: Option<String>,
    /// The discount with this code (as typed: it's normalised).
    code: Option<String>,
}

type DiscountPage = PageResponse<DiscountResponse>;

/// Discounts, newest first.
#[utoipa::path(
    get,
    path = "/admin/discounts",
    params(PageParams, DiscountFilters),
    responses((status = 200, description = "OK", body = DiscountPage)),
    extensions(("x-hypha-scopes" = json!(["discounts:read"]))),
)]
pub async fn list<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    query: ListQuery<DiscountFilters>,
) -> Result<Json<DiscountPage>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let (page, filters, scope) = query.parse()?;
    let now = state.clock.now();
    let filter = DiscountFilter {
        phase: filters.status.map(Into::into),
        method: filters.method.map(Into::into),
        class: filters.class.map(Into::into),
        q: filters.q,
        code: filters.code,
        at: now,
    };
    let page = list_discounts(&ctx, &ports, filter, page).await?;
    let mut items = responses(&ports, &ctx, &page.items, now).await?.into_iter();
    Ok(Json(PageResponse::new(page, &scope, Discount::id, |_| {
        items
            .next()
            .ok_or_else(|| ApiError::internal("a discount without a response"))
    })?))
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateDiscountRequest {
    /// Shown to buyers; 1 to 255 characters.
    title: String,
    /// Fixed for good.
    method: DiscountMethodDto,
    /// Defaults to `active`.
    #[serde(default)]
    status: Option<DiscountStatusDto>,
    /// RFC 3339; `null` for at once.
    #[serde(default)]
    #[schema(value_type = Option<String>, format = DateTime)]
    starts_at: Option<Instant>,
    /// RFC 3339, exclusive; `null` for never. After `starts_at`.
    #[serde(default)]
    #[schema(value_type = Option<String>, format = DateTime)]
    ends_at: Option<Instant>,
    /// All must hold; none for everyone.
    #[serde(default)]
    conditions: Vec<DiscountConditionDto>,
    kind: DiscountKindDto,
    /// Defaults to none: it applies alone.
    #[serde(default)]
    combines_with: Vec<DiscountClassDto>,
    /// Defaults to no limit.
    #[serde(default)]
    limits: DiscountLimitsDto,
    /// Codes to add with it (`method: codes` only), at most 1,000; more through
    /// `POST …/codes` and `…/codes/generate`.
    #[serde(default)]
    codes: Vec<String>,
}

#[allow(clippy::too_many_arguments)]
fn draft_of(
    title: String,
    method: Method,
    status: Option<DiscountStatusDto>,
    starts_at: Option<Instant>,
    ends_at: Option<Instant>,
    conditions: Vec<DiscountConditionDto>,
    kind: DiscountKindDto,
    combines_with: Vec<DiscountClassDto>,
    limits: DiscountLimitsDto,
) -> Result<DiscountDraft, ApiError> {
    Ok(DiscountDraft {
        title,
        method,
        status: status.map_or(Status::Active, Into::into),
        starts_at: starts_at.map(|i| i.0),
        ends_at: ends_at.map(|i| i.0),
        conditions: conditions.into_iter().map(Into::into).collect(),
        kind: kind.into_kind()?,
        combines_with: classes_in(combines_with),
        limits: limits.into(),
    })
}

/// Creates a discount, with its first codes in the same write. "A 10% welcome code" is
/// `{"title": "Welcome", "method": "codes", "kind": {"type": "standard", "rewards":
/// [{"target": {"type": "subtotal"}, "value": {"type": "percent", "basis_points":
/// 1000}}]}, "codes": ["WELCOME10"]}`. Every broken rule is answered at once
/// (`invalid_discount`); at most 100 automatic discounts are active and not expired.
#[utoipa::path(
    post,
    path = "/admin/discounts",
    request_body = CreateDiscountRequest,
    responses((status = 201, description = "Created", body = DiscountResponse)),
    extensions(("x-hypha-scopes" = json!(["discounts:write"]))),
)]
pub async fn create<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    body: JsonBody<CreateDiscountRequest>,
) -> Result<(StatusCode, Json<DiscountResponse>), ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let draft = draft_of(
        req.title,
        req.method.into(),
        req.status,
        req.starts_at,
        req.ends_at,
        req.conditions,
        req.kind,
        req.combines_with,
        req.limits,
    )?;
    let input = CreateDiscount {
        draft,
        codes: req
            .codes
            .into_iter()
            .map(|code| NewCode {
                code,
                usage_limit: None,
            })
            .collect(),
        currencies: currencies(&ports).await?,
    };
    let created = create_discount(&ctx, &ports, &state.ids, input).await?;
    Ok((
        StatusCode::CREATED,
        Json(one(&ports, &ctx, &created, state.clock.now()).await?),
    ))
}

/// A discount.
#[utoipa::path(
    get,
    path = "/admin/discounts/{id}",
    params(("id" = DiscountId, Path)),
    responses((status = 200, description = "OK", body = DiscountResponse)),
    extensions(("x-hypha-scopes" = json!(["discounts:read"]))),
)]
pub async fn fetch<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<DiscountId>,
) -> Result<Json<DiscountResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let d = get_discount(&ctx, &ports, id).await?;
    Ok(Json(one(&ports, &ctx, &d, state.clock.now()).await?))
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateDiscountRequest {
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = String, required = false)]
    title: Option<String>,
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = DiscountStatusDto, required = false)]
    status: Option<DiscountStatusDto>,
    /// `null` clears it.
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<String>, format = DateTime, required = false)]
    starts_at: Option<Option<Instant>>,
    /// `null` clears it.
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<String>, format = DateTime, required = false)]
    ends_at: Option<Option<Instant>>,
    /// Replaces them all; `[]` for none.
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = Vec<DiscountConditionDto>, required = false)]
    conditions: Option<Vec<DiscountConditionDto>>,
    /// Replaces it.
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = DiscountKindDto, required = false)]
    kind: Option<DiscountKindDto>,
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = Vec<DiscountClassDto>, required = false)]
    combines_with: Option<Vec<DiscountClassDto>>,
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = DiscountLimitsDto, required = false)]
    limits: Option<DiscountLimitsDto>,
}

fn patch_of(req: UpdateDiscountRequest) -> Result<DiscountPatch, ApiError> {
    Ok(DiscountPatch {
        title: req.title,
        status: req.status.map(Into::into),
        starts_at: req.starts_at.map(|t| t.map(|i| i.0)),
        ends_at: req.ends_at.map(|t| t.map(|i| i.0)),
        conditions: req
            .conditions
            .map(|c| c.into_iter().map(Into::into).collect()),
        kind: req.kind.map(DiscountKindDto::into_kind).transpose()?,
        combines_with: req.combines_with.map(classes_in),
        limits: req.limits.map(Into::into),
    })
}

/// Changes a discount (JSON Merge Patch); its method is fixed. The result is checked as
/// a whole, like a new one.
#[utoipa::path(
    patch,
    path = "/admin/discounts/{id}",
    params(("id" = DiscountId, Path)),
    request_body = UpdateDiscountRequest,
    responses((status = 200, description = "OK", body = DiscountResponse)),
    extensions(("x-hypha-scopes" = json!(["discounts:write"]))),
)]
pub async fn update<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<DiscountId>,
    body: JsonBody<UpdateDiscountRequest>,
) -> Result<Json<DiscountResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let patch = patch_of(body.parse()?)?;
    let currencies = currencies(&ports).await?;
    let d = update_discount(&ctx, &ports, id, patch, &currencies).await?;
    Ok(Json(one(&ports, &ctx, &d, state.clock.now()).await?))
}

/// What `POST /admin/discounts/summary` reads: a new discount's body (as `POST
/// /admin/discounts` takes it; `codes` are ignored), or a saved discount's `id` with the
/// changes `PATCH` would make to it. The handler reads one or the other by whether `id`
/// is there, for exact errors; this type is the documented shape.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(untagged)]
pub enum DiscountSummaryRequest {
    New(CreateDiscountRequest),
    Saved(SavedDiscountSummaryRequest),
}

/// A saved discount's `id` with the fields `PATCH /admin/discounts/{id}` takes.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SavedDiscountSummaryRequest {
    /// The saved discount the changes go over.
    id: DiscountId,
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = String, required = false)]
    title: Option<String>,
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = DiscountStatusDto, required = false)]
    status: Option<DiscountStatusDto>,
    /// `null` clears it.
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<String>, format = DateTime, required = false)]
    starts_at: Option<Option<Instant>>,
    /// `null` clears it.
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<String>, format = DateTime, required = false)]
    ends_at: Option<Option<Instant>>,
    /// Replaces them all; `[]` for none.
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = Vec<DiscountConditionDto>, required = false)]
    conditions: Option<Vec<DiscountConditionDto>>,
    /// Replaces it.
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = DiscountKindDto, required = false)]
    kind: Option<DiscountKindDto>,
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = Vec<DiscountClassDto>, required = false)]
    combines_with: Option<Vec<DiscountClassDto>>,
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = DiscountLimitsDto, required = false)]
    limits: Option<DiscountLimitsDto>,
}

impl SavedDiscountSummaryRequest {
    fn changes(self) -> UpdateDiscountRequest {
        UpdateDiscountRequest {
            title: self.title,
            status: self.status,
            starts_at: self.starts_at,
            ends_at: self.ends_at,
            conditions: self.conditions,
            kind: self.kind,
            combines_with: self.combines_with,
            limits: self.limits,
        }
    }
}

/// A broken rule, where it is (`kind.rewards[0].value.basis_points`).
#[derive(Serialize, utoipa::ToSchema)]
pub struct DiscountProblemResponse {
    path: String,
    /// As in `invalid_discount`'s `details.problems`, plus `market_not_found` and
    /// `discount_currency_mismatch`.
    code: String,
    message: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct DiscountSummaryResponse {
    /// The sentence the discount would read as; `null` while it breaks a rule.
    summary: Option<String>,
    /// Every rule it breaks; `[]` when it could be saved as is.
    problems: Vec<DiscountProblemResponse>,
}

/// What a discount would read as, and every rule it breaks, writing nothing (no preview
/// token either): for a form that shows its summary as it's typed. A body that isn't a
/// discount at all (a missing `title`, an unknown field) is `422 invalid_body` as on
/// `POST /admin/discounts`.
#[utoipa::path(
    post,
    path = "/admin/discounts/summary",
    request_body = DiscountSummaryRequest,
    responses((status = 200, description = "OK", body = DiscountSummaryResponse)),
    extensions(("x-hypha-scopes" = json!(["discounts:read"]))),
)]
pub async fn summarize_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    body: JsonBody<serde_json::Value>,
) -> Result<Json<DiscountSummaryResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let body: serde_json::Value = body.parse()?;
    let invalid = |e: serde_json::Error| ApiError::invalid_body(e.to_string());
    let draft = match body.get("id") {
        Some(_) => {
            let saved: SavedDiscountSummaryRequest =
                serde_json::from_value(body).map_err(invalid)?;
            SummaryDraft::Saved(saved.id, patch_of(saved.changes())?)
        }
        None => {
            let req: CreateDiscountRequest = serde_json::from_value(body).map_err(invalid)?;
            SummaryDraft::New(draft_of(
                req.title,
                req.method.into(),
                req.status,
                req.starts_at,
                req.ends_at,
                req.conditions,
                req.kind,
                req.combines_with,
                req.limits,
            )?)
        }
    };
    let currencies = currencies(&ports).await?;
    Ok(Json(
        match summarize(&ctx, &ports, draft, &currencies).await? {
            Ok(discount) => DiscountSummaryResponse {
                summary: Some(summary(
                    &discount,
                    None,
                    &staff_names(&ports, &ctx, std::slice::from_ref(&discount)).await?,
                    Audience::Staff,
                )),
                problems: Vec::new(),
            },
            Err(problems) => DiscountSummaryResponse {
                summary: None,
                problems: problems
                    .into_iter()
                    .map(|p| DiscountProblemResponse {
                        path: p.path,
                        code: p.code.to_string(),
                        message: p.message,
                    })
                    .collect(),
            },
        },
    ))
}

/// Deletes a discount with its codes and usage records. Orders keep their copies of what
/// it took off; carts holding its codes show them `invalid`.
#[utoipa::path(
    delete,
    path = "/admin/discounts/{id}",
    params(("id" = DiscountId, Path)),
    responses((status = 204, description = "No content")),
    extensions(("x-hypha-scopes" = json!(["discounts:write"]))),
)]
pub async fn remove<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<DiscountId>,
) -> Result<StatusCode, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    delete_discount(&ctx, &ports, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- codes ----------

#[derive(Serialize, utoipa::ToSchema)]
pub struct DiscountCodeResponse {
    /// As added.
    code: String,
    /// As compared: trimmed, compatibility-normalised and upper-cased. Unique in the
    /// store.
    normalized: String,
    /// Orders it may be used on (1 for a single-use code); `null` for no limit of its own.
    usage_limit: Option<u32>,
    #[schema(minimum = 0)]
    used: u64,
    active: bool,
    /// The label it was added or generated under.
    batch: Option<String>,
    created_at: Option<String>,
}

impl DiscountCodeResponse {
    fn new(c: &DiscountCode) -> Result<Self, ApiError> {
        Ok(Self {
            code: c.code.clone(),
            normalized: c.normalized.clone(),
            usage_limit: c.usage_limit,
            used: c.used,
            active: c.active,
            batch: c.batch.clone(),
            created_at: c.created_at.map(rfc3339).transpose()?,
        })
    }
}

#[derive(Deserialize, utoipa::IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct CodeFilters {
    /// A part of the code, as typed.
    q: Option<String>,
    /// Only those added under this batch label.
    batch: Option<String>,
    /// Only active ones (`true`) or only inactive ones (`false`).
    active: Option<bool>,
}

type DiscountCodePage = PageResponse<DiscountCodeResponse>;

/// A discount's codes, by code.
#[utoipa::path(
    get,
    path = "/admin/discounts/{id}/codes",
    params(("id" = DiscountId, Path), PageParams, CodeFilters),
    responses((status = 200, description = "OK", body = DiscountCodePage)),
    extensions(("x-hypha-scopes" = json!(["discounts:read"]))),
)]
pub async fn codes<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<DiscountId>,
    query: ListQuery<CodeFilters>,
) -> Result<Json<DiscountCodePage>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let (page, filters, scope) = query.parse::<String>()?;
    let filter = CodeFilter {
        q: filters.q,
        batch: filters.batch,
        active: filters.active,
    };
    let page = list_codes(&ctx, &ports, id, filter, page).await?;
    Ok(Json(PageResponse::new(
        page,
        &scope,
        |c| c.normalized.clone(),
        |c| DiscountCodeResponse::new(&c),
    )?))
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DiscountNewCodeDto {
    /// 1 to 64 characters, no spaces; unique in the store ignoring case.
    code: String,
    /// `null` (the default) for no limit of its own; 1 for single use.
    #[serde(default)]
    usage_limit: Option<u32>,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AddCodesRequest {
    /// 1 to 1,000; all are added or none.
    codes: Vec<DiscountNewCodeDto>,
    /// A label to find, export or remove them by.
    #[serde(default)]
    batch: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct AddedCodesResponse {
    /// The codes added, in the order given.
    codes: Vec<DiscountCodeResponse>,
}

fn added(codes: &[DiscountCode]) -> Result<AddedCodesResponse, ApiError> {
    Ok(AddedCodesResponse {
        codes: codes
            .iter()
            .map(DiscountCodeResponse::new)
            .collect::<Result<_, _>>()?,
    })
}

/// Adds codes to a discount by code (`409 discount_automatic` for an automatic one). A
/// code another discount has is `409 discount_code_taken`, and nothing is added.
#[utoipa::path(
    post,
    path = "/admin/discounts/{id}/codes",
    params(("id" = DiscountId, Path)),
    request_body = AddCodesRequest,
    responses((status = 201, description = "Created", body = AddedCodesResponse)),
    extensions(("x-hypha-scopes" = json!(["discounts:write"]))),
)]
pub async fn add<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<DiscountId>,
    body: JsonBody<AddCodesRequest>,
) -> Result<(StatusCode, Json<AddedCodesResponse>), ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let codes = req
        .codes
        .into_iter()
        .map(|c| NewCode {
            code: c.code,
            usage_limit: c.usage_limit,
        })
        .collect();
    let codes = add_codes(&ctx, &ports, id, codes, req.batch).await?;
    Ok((StatusCode::CREATED, Json(added(&codes)?)))
}

fn default_length() -> usize {
    12
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct GenerateCodesRequest {
    /// 1 to 10,000.
    count: usize,
    /// Starts every code (normalised like a code), leaving room for at least 8 random
    /// characters.
    #[serde(default)]
    prefix: Option<String>,
    /// 8 to 24 characters, the prefix included, at least 8 of them after it; defaults to
    /// 12.
    #[serde(default = "default_length")]
    length: usize,
    /// Defaults to 1: single-use codes. `null` for no limit of their own.
    #[serde(default = "single_use")]
    usage_limit: Option<u32>,
    /// A label to find, export or remove them by.
    #[serde(default)]
    batch: Option<String>,
}

fn single_use() -> Option<u32> {
    Some(1)
}

/// Generates codes: random letters and digits without look-alikes (no `0`/`O`, `1`/`I`/
/// `L`), drawn again when taken, all added in one write.
#[utoipa::path(
    post,
    path = "/admin/discounts/{id}/codes/generate",
    params(("id" = DiscountId, Path)),
    request_body = GenerateCodesRequest,
    responses((status = 201, description = "Created", body = AddedCodesResponse)),
    extensions(("x-hypha-scopes" = json!(["discounts:write"]))),
)]
pub async fn generate<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<DiscountId>,
    body: JsonBody<GenerateCodesRequest>,
) -> Result<(StatusCode, Json<AddedCodesResponse>), ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let generation = Generation {
        count: req.count,
        prefix: req.prefix,
        length: req.length,
    };
    let codes = generate_codes(
        &ctx,
        &ports,
        &state.env,
        id,
        generation,
        req.usage_limit,
        req.batch,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(added(&codes)?)))
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoveCodesRequest {
    /// These codes (as typed), at most 1,000.
    #[serde(default)]
    codes: Option<Vec<String>>,
    /// Or every code of this batch.
    #[serde(default)]
    batch: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct RemovedCodesResponse {
    /// How many went.
    #[schema(minimum = 0)]
    removed: u64,
}

/// Removes listed codes or a whole batch: one of `codes` and `batch`.
#[utoipa::path(
    delete,
    path = "/admin/discounts/{id}/codes",
    params(("id" = DiscountId, Path)),
    request_body = RemoveCodesRequest,
    responses((status = 200, description = "OK", body = RemovedCodesResponse)),
    extensions(("x-hypha-scopes" = json!(["discounts:write"]))),
)]
pub async fn remove_codes_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<DiscountId>,
    body: JsonBody<RemoveCodesRequest>,
) -> Result<Json<RemovedCodesResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let selection = match (req.codes, req.batch) {
        (Some(codes), None) => CodeSelection::Codes(codes),
        (None, Some(batch)) => CodeSelection::Batch(batch),
        _ => {
            return Err(ApiError::invalid_body("give one of `codes` and `batch`"));
        }
    };
    let removed = remove_codes(&ctx, &ports, id, selection).await?;
    Ok(Json(RemovedCodesResponse { removed }))
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateCodesRequest {
    /// These codes (as typed), at most 1,000.
    #[serde(default)]
    codes: Option<Vec<String>>,
    /// Or every code of this batch.
    #[serde(default)]
    batch: Option<String>,
    /// `false` deactivates them: carts refuse an inactive code like an unknown one
    /// (`422 discount_code_invalid`), and a cart holding one shows it `invalid`. `true`
    /// makes them usable again.
    active: bool,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct UpdatedCodesResponse {
    /// How many changed (those already as asked aren't counted).
    #[schema(minimum = 0)]
    updated: u64,
}

/// Deactivates or reactivates listed codes or a whole batch: one of `codes` and `batch`.
/// Their uses so far stay.
#[utoipa::path(
    patch,
    path = "/admin/discounts/{id}/codes",
    params(("id" = DiscountId, Path)),
    request_body = UpdateCodesRequest,
    responses((status = 200, description = "OK", body = UpdatedCodesResponse)),
    extensions(("x-hypha-scopes" = json!(["discounts:write"]))),
)]
pub async fn update_codes<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<DiscountId>,
    body: JsonBody<UpdateCodesRequest>,
) -> Result<Json<UpdatedCodesResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let selection = match (req.codes, req.batch) {
        (Some(codes), None) => CodeSelection::Codes(codes),
        (None, Some(batch)) => CodeSelection::Batch(batch),
        _ => {
            return Err(ApiError::invalid_body("give one of `codes` and `batch`"));
        }
    };
    let updated = set_codes_active(&ctx, &ports, id, selection, req.active).await?;
    Ok(Json(UpdatedCodesResponse { updated }))
}

/// Codes per page of the export: the export reads every code, page by page.
const EXPORT_PAGE: u32 = 100;

/// Every code of a discount as CSV (`code,normalized,usage_limit,used,active,batch,
/// created_at`), by code; `batch` narrows it.
#[utoipa::path(
    get,
    path = "/admin/discounts/{id}/codes/export",
    params(("id" = DiscountId, Path), ("batch" = Option<String>, Query, description = "Only this batch's codes.")),
    responses((status = 200, description = "The codes", content_type = "text/csv", body = String)),
    extensions(("x-hypha-scopes" = json!(["discounts:read"]))),
)]
pub async fn export<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<DiscountId>,
    query: ListQuery<ExportFilters>,
) -> Result<Response, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let (_, filters, _) = query.parse::<String>()?;
    let filter = CodeFilter {
        q: None,
        batch: filters.batch,
        active: None,
    };
    let mut csv = String::from("code,normalized,usage_limit,used,active,batch,created_at\n");
    let mut after: Option<String> = None;
    loop {
        let page = PageRequest {
            after: after.take(),
            limit: PageLimit::new(EXPORT_PAGE).map_err(ApiError::internal)?,
        };
        let found = list_codes(&ctx, &ports, id, filter.clone(), page).await?;
        for c in &found.items {
            let created = c.created_at.map(rfc3339).transpose()?.unwrap_or_default();
            csv.push_str(&format!(
                "{},{},{},{},{},{},{}\n",
                csv_field(&c.code),
                csv_field(&c.normalized),
                c.usage_limit.map(|n| n.to_string()).unwrap_or_default(),
                c.used,
                c.active,
                csv_field(c.batch.as_deref().unwrap_or("")),
                created
            ));
        }
        if !found.has_more {
            break;
        }
        after = found.items.last().map(|c| c.normalized.clone());
    }
    Response::builder()
        .header(header::CONTENT_TYPE, "text/csv; charset=utf-8")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"discount-codes-{id}.csv\""),
        )
        .header(header::CACHE_CONTROL, "private, no-store")
        .body(Body::from(csv))
        .map_err(ApiError::internal)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportFilters {
    batch: Option<String>,
}

/// A CSV field, quoted when it holds a comma, a quote or a line break, and with a leading
/// `=`, `+`, `-` or `@` escaped so a spreadsheet doesn't run it as a formula.
fn csv_field(value: &str) -> String {
    let value = if value.starts_with(['=', '+', '-', '@']) {
        format!("'{value}")
    } else {
        value.to_string()
    };
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value
    }
}

// ---------- usage ----------

/// A use of a discount. The customer's names and email are personal data: callers other
/// than staff sessions see them only with `customer_data:name` / `customer_data:email`,
/// else they're absent and listed in `redacted`.
#[derive(Serialize, utoipa::ToSchema)]
pub struct DiscountUsageResponse {
    order: OrderId,
    /// The order's number (`1001`).
    #[schema(minimum = 1)]
    order_number: Option<i64>,
    customer: CustomerId,
    /// The customer's now; `null` when unknown.
    first_name: Option<String>,
    last_name: Option<String>,
    email: Option<String>,
    /// The code it came by, normalised; `null` for an automatic discount.
    code: Option<String>,
    /// What it took off the order.
    amount: Money,
    used_at: String,
    /// The personal fields left out for this caller (absent from the object then); never
    /// sent to staff.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    redacted: Option<Vec<String>>,
}

impl DiscountUsageResponse {
    fn new(
        u: &Usage,
        number: Option<OrderNumber>,
        customer: Option<&CustomerSummary>,
    ) -> Result<Self, ApiError> {
        Ok(Self {
            order: u.order,
            order_number: number.map(|n| n.value()),
            customer: u.customer,
            first_name: customer.and_then(|c| c.first_name.clone()),
            last_name: customer.and_then(|c| c.last_name.clone()),
            email: customer.map(|c| c.email.as_str().to_string()),
            code: u.code.clone(),
            amount: u.amount,
            used_at: rfc3339(u.at)?,
            redacted: None,
        })
    }
}

type DiscountUsagePage = PageResponse<DiscountUsageResponse>;

/// The orders a discount was used on, newest first, with each order's number and its
/// customer's names and email: it needs reading orders and customers too.
#[utoipa::path(
    get,
    path = "/admin/discounts/{id}/usage",
    params(("id" = DiscountId, Path), PageParams),
    responses((status = 200, description = "OK", body = DiscountUsagePage)),
    extensions(("x-hypha-scopes" = json!(["discounts:read", "customers:read", "orders:read"]))),
)]
pub async fn usage<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<DiscountId>,
    query: ListQuery,
) -> Result<Json<Redacted>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let (page, _, scope) = query.parse::<OrderId>()?;
    let page = list_usage(&ctx, &ports, id, page).await?;
    let orders: Vec<OrderId> = page.items.iter().map(|u| u.order).collect();
    let numbers = ports
        .orders()
        .numbers(&orders)
        .await
        .map_err(hypha_discounts::CommandError::from)?;
    let customers = customers_by_id(&ports, page.items.iter().map(|u| u.customer)).await?;
    let page = PageResponse::new(
        page,
        &scope,
        |u| u.order,
        |u| {
            DiscountUsageResponse::new(
                &u,
                numbers.get(&u.order).copied(),
                customers.get(&u.customer),
            )
        },
    )?;
    redact::page(&ctx, Family::DiscountUsage, page).map(Json)
}

/// The summaries of `ids`' customers, by ID: at most a page of them.
async fn customers_by_id<P: HasCustomers>(
    ports: &P,
    ids: impl Iterator<Item = CustomerId>,
) -> Result<HashMap<CustomerId, CustomerSummary>, ApiError> {
    let ids: Vec<CustomerId> = ids.collect();
    let filter = CustomerFilter::default()
        .only(ids)
        .map_err(ApiError::internal)?;
    let Ok(limit) = PageLimit::new(filter.ids().len() as u32) else {
        return Ok(HashMap::new());
    };
    let found = ports
        .customers()
        .list(
            &filter,
            CustomerSort::Newest,
            PageRequest { after: None, limit },
        )
        .await
        .map_err(hypha_discounts::CommandError::from)?;
    Ok(found.items.into_iter().map(|c| (c.id, c)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_csv_field_with_a_comma_is_quoted() {
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
    }

    #[test]
    fn a_csv_field_that_reads_as_a_formula_is_escaped() {
        assert_eq!(csv_field("=SUM(A1)"), "'=SUM(A1)");
        assert_eq!(csv_field("WELCOME"), "WELCOME");
    }
}

// ---------- promotions ----------

/// A promotion a product shows: a live automatic discount that may reach it, whose
/// conditions are only about market, channel and the items. A badge ("Buy 2, get 1
/// free"), never a price: what a shopper pays is the cart's.
#[derive(Serialize, Clone, utoipa::ToSchema)]
pub struct PromotionResponse {
    discount: DiscountId,
    title: String,
    /// One sentence from its data, amounts in the shopper's market.
    summary: String,
}

/// The promotions of each of `products` in `market` and `channel` now, in two reads at
/// most (the live automatic discounts, and the products' categories when one names
/// categories). A product without any is left out.
pub(crate) async fn promotions_of<
    P: hypha_discounts::HasDiscounts + hypha_markets::HasPriceLists,
>(
    ports: &P,
    products: &[ProductId],
    market: MarketId,
    channel: Option<hypha_core::ChannelId>,
    at: hypha_core::Timestamp,
) -> Result<HashMap<ProductId, Vec<PromotionResponse>>, ApiError> {
    use hypha_markets::PriceListRepo;
    let automatic = ports
        .discounts()
        .live_automatic(at)
        .await
        .map_err(hypha_discounts::CommandError::from)?;
    if automatic.is_empty() || products.is_empty() {
        return Ok(HashMap::new());
    }
    let context = hypha_discounts::DiscountContext {
        automatic,
        ..Default::default()
    };
    let categories = if context.names_categories() {
        ports
            .price_lists()
            .product_categories(products)
            .await
            .map_err(hypha_discounts::CommandError::from)?
    } else {
        HashMap::new()
    };
    let request = NameRequest::for_shoppers(&context.automatic);
    let names = if request.is_empty() {
        Names::new()
    } else {
        ports
            .discounts()
            .names(&request)
            .await
            .map_err(hypha_discounts::CommandError::from)?
    };
    let none = std::collections::HashSet::new();
    let mut found = HashMap::new();
    for product in products {
        let shown: Vec<PromotionResponse> = hypha_discounts::promotions(
            &context.automatic,
            *product,
            categories.get(product).unwrap_or(&none),
            market,
            channel,
            at,
            &names,
        )
        .into_iter()
        .map(|p| PromotionResponse {
            discount: p.discount,
            title: p.title,
            summary: p.summary,
        })
        .collect();
        if !shown.is_empty() {
            found.insert(*product, shown);
        }
    }
    Ok(found)
}
