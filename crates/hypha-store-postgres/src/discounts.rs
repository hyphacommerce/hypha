//! Discounts, their codes and their usage (`0083_discounts.sql`). A discount's conditions
//! and kind are one JSON document, `rules`, in the storage shape below: a contract with
//! stored rows, so a field is added, never renamed.

use std::collections::{BTreeMap, HashMap, HashSet};

use hypha_core::{
    BasisPoints, CategoryId, ChannelId, CustomerGroupId, CustomerId, DiscountId, MarketId, Money,
    OrderId, Page, PageLimit, PageRequest, ProductId, Recorded, RepoError, Timestamp, VariantId,
    Version, Versioned, stamp_insert, stamp_update,
};
use hypha_discounts::{
    Allocation, Class, ClassSet, CodeFilter, CodeSelection, Condition, Discount, DiscountCode,
    DiscountFilter, DiscountRepo, DiscountSnapshot, ItemSet, Items, Kind, Limits, Measure, Method,
    PerMarket, Phase, Reward, Status, Target, Threshold, Tier, Usage, UsageClaim, Value, keys,
};
use serde::{Deserialize, Serialize};
use tokio_postgres::{Row, types::Type};
use uuid::Uuid;

use crate::{
    columns::{
        Param, col, currency_from_db, micros, opt_micros, opt_ts, stamp_columns, stamps_from_row,
        ts, version_from_db, version_to_db, write_stamps,
    },
    error::{corrupt, repo_error},
    outbox::insert_events,
    store::{PgConnection, PgStore, Queries, in_transaction},
};

// ---------- the stored rules ----------

type AmountsJson = BTreeMap<Uuid, Money>;

#[derive(Serialize, Deserialize)]
struct RulesJson {
    conditions: Vec<ConditionJson>,
    kind: KindJson,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ConditionJson {
    Markets { ids: Vec<Uuid> },
    Channels { ids: Vec<Uuid> },
    CustomerGroups { ids: Vec<Uuid> },
    Customers { ids: Vec<Uuid> },
    SignedIn,
    MinSubtotal { amounts: AmountsJson },
    MinQuantity { quantity: u32 },
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ItemsJson {
    All,
    Products { ids: Vec<Uuid> },
    Variants { ids: Vec<Uuid> },
    Categories { ids: Vec<Uuid> },
}

#[derive(Serialize, Deserialize)]
struct ItemSetJson {
    items: ItemsJson,
    exclude_on_sale: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AllocationJson {
    Each,
    Across,
    Cheapest { units: u32 },
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ValueJson {
    Percent {
        basis_points: i32,
    },
    Amount {
        amounts: AmountsJson,
        allocation: AllocationJson,
    },
    FixedPrice {
        amounts: AmountsJson,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum TargetJson {
    Items { items: ItemSetJson },
    Subtotal,
    Shipping { max_amount: Option<AmountsJson> },
}

#[derive(Serialize, Deserialize)]
struct RewardJson {
    target: TargetJson,
    value: ValueJson,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ThresholdJson {
    Quantity { quantity: u32 },
    Amount { amounts: AmountsJson },
}

#[derive(Serialize, Deserialize)]
struct TierJson {
    threshold: ThresholdJson,
    value: ValueJson,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum KindJson {
    Standard {
        rewards: Vec<RewardJson>,
    },
    BuyXGetY {
        buys: ItemSetJson,
        buy: ThresholdJson,
        gets: ItemSetJson,
        get_quantity: u32,
        value: ValueJson,
        uses_per_order: Option<u32>,
    },
    Tiered {
        items: ItemSetJson,
        measure: String,
        tiers: Vec<TierJson>,
    },
}

fn uuids<T>(ids: &[T], f: impl Fn(&T) -> Uuid) -> Vec<Uuid> {
    ids.iter().map(f).collect()
}

fn amounts_out(p: &PerMarket) -> AmountsJson {
    p.iter().map(|(m, a)| (m.as_uuid(), a)).collect()
}

fn amounts_in(a: AmountsJson) -> PerMarket {
    PerMarket::new(
        a.into_iter()
            .map(|(m, a)| (MarketId::from_uuid(m), a))
            .collect(),
    )
}

fn items_out(s: &ItemSet) -> ItemSetJson {
    ItemSetJson {
        items: match &s.items {
            Items::All => ItemsJson::All,
            Items::Products(ids) => ItemsJson::Products {
                ids: uuids(ids, ProductId::as_uuid),
            },
            Items::Variants(ids) => ItemsJson::Variants {
                ids: uuids(ids, VariantId::as_uuid),
            },
            Items::Categories(ids) => ItemsJson::Categories {
                ids: uuids(ids, CategoryId::as_uuid),
            },
        },
        exclude_on_sale: s.exclude_on_sale,
    }
}

fn items_in(s: ItemSetJson) -> ItemSet {
    ItemSet {
        items: match s.items {
            ItemsJson::All => Items::All,
            ItemsJson::Products { ids } => {
                Items::Products(ids.into_iter().map(ProductId::from_uuid).collect())
            }
            ItemsJson::Variants { ids } => {
                Items::Variants(ids.into_iter().map(VariantId::from_uuid).collect())
            }
            ItemsJson::Categories { ids } => {
                Items::Categories(ids.into_iter().map(CategoryId::from_uuid).collect())
            }
        },
        exclude_on_sale: s.exclude_on_sale,
    }
}

fn value_out(v: &Value) -> ValueJson {
    match v {
        Value::Percent(bp) => ValueJson::Percent {
            basis_points: bp.value(),
        },
        Value::Amount {
            amounts,
            allocation,
        } => ValueJson::Amount {
            amounts: amounts_out(amounts),
            allocation: match allocation {
                Allocation::Each => AllocationJson::Each,
                Allocation::Across => AllocationJson::Across,
                Allocation::Cheapest { units } => AllocationJson::Cheapest { units: *units },
            },
        },
        Value::FixedPrice(amounts) => ValueJson::FixedPrice {
            amounts: amounts_out(amounts),
        },
    }
}

fn value_in(v: ValueJson) -> Result<Value, RepoError> {
    Ok(match v {
        ValueJson::Percent { basis_points } => {
            Value::Percent(BasisPoints::new(basis_points).map_err(corrupt)?)
        }
        ValueJson::Amount {
            amounts,
            allocation,
        } => Value::Amount {
            amounts: amounts_in(amounts),
            allocation: match allocation {
                AllocationJson::Each => Allocation::Each,
                AllocationJson::Across => Allocation::Across,
                AllocationJson::Cheapest { units } => Allocation::Cheapest { units },
            },
        },
        ValueJson::FixedPrice { amounts } => Value::FixedPrice(amounts_in(amounts)),
    })
}

fn threshold_out(t: &Threshold) -> ThresholdJson {
    match t {
        Threshold::Quantity(n) => ThresholdJson::Quantity { quantity: *n },
        Threshold::Amount(a) => ThresholdJson::Amount {
            amounts: amounts_out(a),
        },
    }
}

fn threshold_in(t: ThresholdJson) -> Threshold {
    match t {
        ThresholdJson::Quantity { quantity } => Threshold::Quantity(quantity),
        ThresholdJson::Amount { amounts } => Threshold::Amount(amounts_in(amounts)),
    }
}

fn rules_out(s: &DiscountSnapshot) -> String {
    let conditions = s
        .conditions
        .iter()
        .map(|c| match c {
            Condition::Markets(ids) => ConditionJson::Markets {
                ids: uuids(ids, MarketId::as_uuid),
            },
            Condition::Channels(ids) => ConditionJson::Channels {
                ids: uuids(ids, ChannelId::as_uuid),
            },
            Condition::CustomerGroups(ids) => ConditionJson::CustomerGroups {
                ids: uuids(ids, CustomerGroupId::as_uuid),
            },
            Condition::Customers(ids) => ConditionJson::Customers {
                ids: uuids(ids, CustomerId::as_uuid),
            },
            Condition::SignedIn => ConditionJson::SignedIn,
            Condition::MinSubtotal(a) => ConditionJson::MinSubtotal {
                amounts: amounts_out(a),
            },
            Condition::MinQuantity(n) => ConditionJson::MinQuantity { quantity: *n },
        })
        .collect();
    let kind = match &s.kind {
        Kind::Standard { rewards } => KindJson::Standard {
            rewards: rewards
                .iter()
                .map(|r| RewardJson {
                    target: match &r.target {
                        Target::Items(set) => TargetJson::Items {
                            items: items_out(set),
                        },
                        Target::Subtotal => TargetJson::Subtotal,
                        Target::Shipping { max_amount } => TargetJson::Shipping {
                            max_amount: max_amount.as_ref().map(amounts_out),
                        },
                    },
                    value: value_out(&r.value),
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
        } => KindJson::BuyXGetY {
            buys: items_out(buys),
            buy: threshold_out(buy),
            gets: items_out(gets),
            get_quantity: *get_quantity,
            value: value_out(value),
            uses_per_order: *uses_per_order,
        },
        Kind::Tiered {
            items,
            measure,
            tiers,
        } => KindJson::Tiered {
            items: items_out(items),
            measure: measure.as_str().to_string(),
            tiers: tiers
                .iter()
                .map(|t| TierJson {
                    threshold: threshold_out(&t.threshold),
                    value: value_out(&t.value),
                })
                .collect(),
        },
    };
    // Plain structs of IDs, numbers and strings: serializing can't fail.
    serde_json::to_string(&RulesJson { conditions, kind }).unwrap_or_default()
}

fn rules_in(text: &str) -> Result<(Vec<Condition>, Kind), RepoError> {
    let rules: RulesJson =
        serde_json::from_str(text).map_err(|e| corrupt(format!("discount rules: {e}")))?;
    let conditions = rules
        .conditions
        .into_iter()
        .map(|c| match c {
            ConditionJson::Markets { ids } => {
                Condition::Markets(ids.into_iter().map(MarketId::from_uuid).collect())
            }
            ConditionJson::Channels { ids } => {
                Condition::Channels(ids.into_iter().map(ChannelId::from_uuid).collect())
            }
            ConditionJson::CustomerGroups { ids } => {
                Condition::CustomerGroups(ids.into_iter().map(CustomerGroupId::from_uuid).collect())
            }
            ConditionJson::Customers { ids } => {
                Condition::Customers(ids.into_iter().map(CustomerId::from_uuid).collect())
            }
            ConditionJson::SignedIn => Condition::SignedIn,
            ConditionJson::MinSubtotal { amounts } => Condition::MinSubtotal(amounts_in(amounts)),
            ConditionJson::MinQuantity { quantity } => Condition::MinQuantity(quantity),
        })
        .collect();
    let kind = match rules.kind {
        KindJson::Standard { rewards } => Kind::Standard {
            rewards: rewards
                .into_iter()
                .map(|r| {
                    Ok(Reward {
                        target: match r.target {
                            TargetJson::Items { items } => Target::Items(items_in(items)),
                            TargetJson::Subtotal => Target::Subtotal,
                            TargetJson::Shipping { max_amount } => Target::Shipping {
                                max_amount: max_amount.map(amounts_in),
                            },
                        },
                        value: value_in(r.value)?,
                    })
                })
                .collect::<Result<_, RepoError>>()?,
        },
        KindJson::BuyXGetY {
            buys,
            buy,
            gets,
            get_quantity,
            value,
            uses_per_order,
        } => Kind::BuyXGetY {
            buys: items_in(buys),
            buy: threshold_in(buy),
            gets: items_in(gets),
            get_quantity,
            value: value_in(value)?,
            uses_per_order,
        },
        KindJson::Tiered {
            items,
            measure,
            tiers,
        } => Kind::Tiered {
            items: items_in(items),
            measure: match measure.as_str() {
                "quantity" => Measure::Quantity,
                "subtotal" => Measure::Subtotal,
                other => return Err(corrupt(format!("tier measure {other:?}"))),
            },
            tiers: tiers
                .into_iter()
                .map(|t| {
                    Ok(Tier {
                        threshold: threshold_in(t.threshold),
                        value: value_in(t.value)?,
                    })
                })
                .collect::<Result<_, RepoError>>()?,
        },
    };
    Ok((conditions, kind))
}

fn classes_out(set: ClassSet) -> Vec<&'static str> {
    set.classes().into_iter().map(Class::as_str).collect()
}

fn classes_in(names: Vec<String>) -> Result<ClassSet, RepoError> {
    let classes = names
        .iter()
        .map(|n| Class::parse(n).ok_or_else(|| corrupt(format!("discount class {n:?}"))))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ClassSet::of(&classes))
}

// ---------- rows ----------

fn columns(prefix: &str) -> String {
    format!(
        "{prefix}id, {prefix}version, {prefix}title, {prefix}method, {prefix}status, {}, {}, \
         {prefix}combines_with, {prefix}rules::text AS rules, {prefix}usage_limit, \
         {prefix}once_per_customer, {}",
        micros(&format!("{prefix}starts_at")),
        micros(&format!("{prefix}ends_at")),
        stamp_columns(prefix)
    )
}

fn discount_from_row(row: &Row) -> Result<Discount, RepoError> {
    let id = DiscountId::from_uuid(col(row, "id")?);
    let bad = |e: &dyn std::fmt::Display| corrupt(format!("discount {id}: {e}"));
    let method: String = col(row, "method")?;
    let status: String = col(row, "status")?;
    let rules: String = col(row, "rules")?;
    let (conditions, kind) = rules_in(&rules)?;
    let usage_limit: Option<i32> = col(row, "usage_limit")?;
    Discount::restore(DiscountSnapshot {
        id,
        version: version_from_db(col(row, "version")?)?,
        stamps: stamps_from_row(row)?,
        title: col(row, "title")?,
        method: Method::parse(&method).ok_or_else(|| bad(&format!("method {method:?}")))?,
        status: Status::parse(&status).ok_or_else(|| bad(&format!("status {status:?}")))?,
        starts_at: opt_ts(col(row, "starts_at")?),
        ends_at: opt_ts(col(row, "ends_at")?),
        conditions,
        kind,
        combines_with: classes_in(col(row, "combines_with")?)?,
        limits: Limits {
            total: usage_limit
                .map(|n| u32::try_from(n).map_err(|e| bad(&e)))
                .transpose()?,
            once_per_customer: col(row, "once_per_customer")?,
        },
    })
    .map_err(|e| bad(&e))
}

fn limit_to_db(limit: Option<u32>) -> Result<Option<i32>, RepoError> {
    limit
        .map(|n| i32::try_from(n).map_err(|_| corrupt(format!("usage limit {n}"))))
        .transpose()
}

/// A new discount and its usage counter, in the caller's transaction.
pub(crate) async fn insert_discount(
    tx: &impl Queries,
    s: &DiscountSnapshot,
    classes: ClassSet,
) -> Result<(), RepoError> {
    let rules = rules_out(s);
    let limit = limit_to_db(s.limits.total)?;
    let combines = classes_out(s.combines_with);
    let classes = classes_out(classes);
    let (starts, ends) = (opt_micros(s.starts_at), opt_micros(s.ends_at));
    tx.query_typed(
        &format!(
            "INSERT INTO discounts (id, version, title, method, status, starts_at, ends_at,
                 classes, combines_with, rules, usage_limit, once_per_customer)
             VALUES ($1, 1, $2, $3, $4, {}, {}, $7, $8, $9::text::jsonb, $10, $11)",
            ts(5),
            ts(6)
        ),
        &[
            (&s.id.as_uuid(), Type::UUID),
            (&s.title, Type::TEXT),
            (&s.method.as_str(), Type::TEXT),
            (&s.status.as_str(), Type::TEXT),
            (&starts, Type::INT8),
            (&ends, Type::INT8),
            (&classes, Type::TEXT_ARRAY),
            (&combines, Type::TEXT_ARRAY),
            (&rules, Type::TEXT),
            (&limit, Type::INT4),
            (&s.limits.once_per_customer, Type::BOOL),
        ],
    )
    .await
    .map_err(repo_error)?;
    tx.query_typed(
        "INSERT INTO discount_usage_counts (discount_id) VALUES ($1)",
        &[(&s.id.as_uuid(), Type::UUID)],
    )
    .await
    .map_err(repo_error)?;
    write_stamps(tx, "discounts", "id", &s.id.as_uuid(), Type::UUID, s.stamps).await
}

async fn update_discount(
    tx: &impl Queries,
    s: &DiscountSnapshot,
    classes: ClassSet,
) -> Result<Version, RepoError> {
    let rules = rules_out(s);
    let limit = limit_to_db(s.limits.total)?;
    let combines = classes_out(s.combines_with);
    let classes = classes_out(classes);
    let (starts, ends) = (opt_micros(s.starts_at), opt_micros(s.ends_at));
    let row = tx
        .query_typed_opt(
            &format!(
                "UPDATE discounts SET version = version + 1, title = $3, status = $4,
                     starts_at = {}, ends_at = {}, classes = $7, combines_with = $8,
                     rules = $9::text::jsonb, usage_limit = $10, once_per_customer = $11
                 WHERE id = $1 AND version = $2 RETURNING version",
                ts(5),
                ts(6)
            ),
            &[
                (&s.id.as_uuid(), Type::UUID),
                (&version_to_db(s.version)?, Type::INT8),
                (&s.title, Type::TEXT),
                (&s.status.as_str(), Type::TEXT),
                (&starts, Type::INT8),
                (&ends, Type::INT8),
                (&classes, Type::TEXT_ARRAY),
                (&combines, Type::TEXT_ARRAY),
                (&rules, Type::TEXT),
                (&limit, Type::INT4),
                (&s.limits.once_per_customer, Type::BOOL),
            ],
        )
        .await
        .map_err(repo_error)?
        .ok_or(RepoError::StaleVersion)?;
    let version = version_from_db(col(&row, "version")?)?;
    write_stamps(tx, "discounts", "id", &s.id.as_uuid(), Type::UUID, s.stamps).await?;
    Ok(version)
}

/// Adds codes in one statement; a taken one fails it (`Duplicate(DISCOUNT_CODE)`), a
/// missing discount its reference.
pub(crate) async fn add_codes(tx: &impl Queries, codes: &[DiscountCode]) -> Result<(), RepoError> {
    if codes.is_empty() {
        return Ok(());
    }
    let normalized: Vec<&str> = codes.iter().map(|c| c.normalized.as_str()).collect();
    let discounts: Vec<Uuid> = codes.iter().map(|c| c.discount.as_uuid()).collect();
    let texts: Vec<&str> = codes.iter().map(|c| c.code.as_str()).collect();
    let limits: Vec<Option<i32>> = codes
        .iter()
        .map(|c| limit_to_db(c.usage_limit))
        .collect::<Result<_, _>>()?;
    let actives: Vec<bool> = codes.iter().map(|c| c.active).collect();
    let batches: Vec<Option<&str>> = codes.iter().map(|c| c.batch.as_deref()).collect();
    tx.query_typed(
        "INSERT INTO discount_codes (normalized, discount_id, code, usage_limit, active, batch)
         SELECT * FROM unnest($1::text[], $2::uuid[], $3::text[], $4::int4[], $5::bool[],
             $6::text[])",
        &[
            (&normalized, Type::TEXT_ARRAY),
            (&discounts, Type::UUID_ARRAY),
            (&texts, Type::TEXT_ARRAY),
            (&limits, Type::INT4_ARRAY),
            (&actives, Type::BOOL_ARRAY),
            (&batches, Type::TEXT_ARRAY),
        ],
    )
    .await
    .map_err(|e| match repo_error(e) {
        // The discount reference: the caller broke the precondition.
        RepoError::Backend(m) if m.contains("foreign key") => {
            RepoError::Backend(format!("a code's discount does not exist: {m}"))
        }
        other => other,
    })?;
    Ok(())
}

pub(crate) async fn remove_codes(
    tx: &impl Queries,
    discount: DiscountId,
    selection: &CodeSelection,
) -> Result<(), RepoError> {
    let id = discount.as_uuid();
    match selection {
        CodeSelection::Codes(codes) => {
            tx.query_typed(
                "DELETE FROM discount_codes WHERE discount_id = $1 AND normalized = ANY($2)",
                &[(&id, Type::UUID), (codes, Type::TEXT_ARRAY)],
            )
            .await
        }
        CodeSelection::Batch(batch) => {
            tx.query_typed(
                "DELETE FROM discount_codes WHERE discount_id = $1 AND batch = $2",
                &[(&id, Type::UUID), (batch, Type::TEXT)],
            )
            .await
        }
    }
    .map_err(repo_error)?;
    Ok(())
}

pub(crate) async fn set_codes_active(
    tx: &impl Queries,
    discount: DiscountId,
    selection: &CodeSelection,
    active: bool,
) -> Result<(), RepoError> {
    let id = discount.as_uuid();
    match selection {
        CodeSelection::Codes(codes) => {
            tx.query_typed(
                "UPDATE discount_codes SET active = $3
                 WHERE discount_id = $1 AND normalized = ANY($2)",
                &[
                    (&id, Type::UUID),
                    (codes, Type::TEXT_ARRAY),
                    (&active, Type::BOOL),
                ],
            )
            .await
        }
        CodeSelection::Batch(batch) => {
            tx.query_typed(
                "UPDATE discount_codes SET active = $3 WHERE discount_id = $1 AND batch = $2",
                &[
                    (&id, Type::UUID),
                    (batch, Type::TEXT),
                    (&active, Type::BOOL),
                ],
            )
            .await
        }
    }
    .map_err(repo_error)?;
    Ok(())
}

/// Counts a use: each counter moves only under its limit, or the claim fails
/// (`Duplicate(DISCOUNT_USAGE_LIMIT)`); the usage row's unique index refuses a second use
/// by one customer of a once-per-customer discount.
pub(crate) async fn claim_usage(tx: &impl Queries, claim: &UsageClaim) -> Result<(), RepoError> {
    let id = claim.discount.as_uuid();
    let limit: Option<i64> = claim.limit.map(i64::from);
    let moved = tx
        .query_typed_opt(
            "UPDATE discount_usage_counts SET used = used + 1
             WHERE discount_id = $1 AND ($2::bigint IS NULL OR used < $2) RETURNING used",
            &[(&id, Type::UUID), (&limit, Type::INT8)],
        )
        .await
        .map_err(repo_error)?;
    if moved.is_none() {
        return Err(RepoError::Duplicate(keys::DISCOUNT_USAGE_LIMIT));
    }
    if let Some((code, code_limit)) = &claim.code {
        let code_limit: Option<i64> = code_limit.map(i64::from);
        let moved = tx
            .query_typed_opt(
                "UPDATE discount_codes SET used = used + 1
                 WHERE normalized = $1 AND discount_id = $2
                     AND ($3::bigint IS NULL OR used < $3) RETURNING used",
                &[
                    (code, Type::TEXT),
                    (&id, Type::UUID),
                    (&code_limit, Type::INT8),
                ],
            )
            .await
            .map_err(repo_error)?;
        if moved.is_none() {
            return Err(RepoError::Duplicate(keys::DISCOUNT_USAGE_LIMIT));
        }
    }
    let code = claim.code.as_ref().map(|(c, _)| c.as_str());
    tx.query_typed(
        &format!(
            "INSERT INTO discount_usages (discount_id, order_id, customer_id, code,
                 once_per_customer, used_at, amount, currency)
             VALUES ($1, $2, $3, $4, $5, {}, $7, $8)",
            ts(6)
        ),
        &[
            (&id, Type::UUID),
            (&claim.order.as_uuid(), Type::UUID),
            (&claim.customer.as_uuid(), Type::UUID),
            (&code, Type::TEXT),
            (&claim.once_per_customer, Type::BOOL),
            (&claim.at.unix_micros(), Type::INT8),
            (&claim.amount.amount(), Type::INT8),
            (&claim.amount.currency().as_str(), Type::TEXT),
        ],
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}

fn code_columns() -> String {
    format!(
        "normalized, discount_id, code, usage_limit, active, batch, used, {}",
        micros("created_at")
    )
}

fn code_from_row(row: &Row) -> Result<DiscountCode, RepoError> {
    let limit: Option<i32> = col(row, "usage_limit")?;
    let used: i64 = col(row, "used")?;
    Ok(DiscountCode {
        discount: DiscountId::from_uuid(col(row, "discount_id")?),
        code: col(row, "code")?,
        normalized: col(row, "normalized")?,
        usage_limit: limit
            .map(|n| u32::try_from(n).map_err(corrupt))
            .transpose()?,
        active: col(row, "active")?,
        batch: col(row, "batch")?,
        used: u64::try_from(used).map_err(corrupt)?,
        created_at: Some(Timestamp::from_unix_micros(col(row, "created_at")?)),
    })
}

fn overfetch(limit: PageLimit) -> i64 {
    i64::from(limit.get()) + 1
}

/// The SQL of a phase at the timestamp in parameter `$at`.
fn phase_sql(phase: Phase, at: &str) -> String {
    match phase {
        Phase::Draft => "status = 'draft'".to_string(),
        Phase::Scheduled => format!("status = 'active' AND starts_at > {at}"),
        Phase::Active => format!(
            "status = 'active' AND (starts_at IS NULL OR starts_at <= {at})
             AND (ends_at IS NULL OR ends_at > {at})"
        ),
        Phase::Expired => format!(
            "status = 'active' AND (starts_at IS NULL OR starts_at <= {at}) AND ends_at <= {at}"
        ),
    }
}

impl<C: PgConnection> DiscountRepo for PgStore<C> {
    async fn get(&self, id: DiscountId) -> Result<Option<Discount>, RepoError> {
        let mut conn = self.conn().await;
        conn.db()
            .query_typed_opt(
                &format!("SELECT {} FROM discounts WHERE id = $1", columns("")),
                &[(&id.as_uuid(), Type::UUID)],
            )
            .await
            .map_err(repo_error)?
            .as_ref()
            .map(discount_from_row)
            .transpose()
    }

    async fn get_many(&self, ids: &[DiscountId]) -> Result<Vec<Discount>, RepoError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let ids = uuids(ids, DiscountId::as_uuid);
        let mut conn = self.conn().await;
        conn.db()
            .query_typed(
                &format!(
                    "SELECT {} FROM discounts WHERE id = ANY($1) ORDER BY id",
                    columns("")
                ),
                &[(&ids, Type::UUID_ARRAY)],
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(discount_from_row)
            .collect()
    }

    async fn list(
        &self,
        filter: &DiscountFilter,
        page: PageRequest<DiscountId>,
    ) -> Result<Page<Discount>, RepoError> {
        let at = filter.at.unix_micros();
        let method = filter.method.map(Method::as_str);
        let class = filter.class.map(Class::as_str);
        let after = page.after.map(|id| id.as_uuid());
        let fetch = overfetch(page.limit);
        let mut params: Vec<Param<'_>> = Vec::new();
        let mut conditions: Vec<String> = Vec::new();
        if let Some(phase) = filter.phase {
            params.push((&at, Type::INT8));
            conditions.push(phase_sql(phase, &ts(params.len())));
        }
        if let Some(method) = &method {
            params.push((method, Type::TEXT));
            conditions.push(format!("method = ${}", params.len()));
        }
        if let Some(class) = &class {
            params.push((class, Type::TEXT));
            conditions.push(format!("${} = ANY(classes)", params.len()));
        }
        if let Some(q) = &filter.q {
            params.push((q, Type::TEXT));
            let n = params.len();
            conditions.push(format!(
                "(strpos(lower(title), lower(${n})) > 0 OR EXISTS (
                     SELECT 1 FROM discount_codes c
                     WHERE c.discount_id = discounts.id AND strpos(c.normalized, upper(${n})) > 0))"
            ));
        }
        if let Some(code) = &filter.code {
            params.push((code, Type::TEXT));
            conditions.push(format!(
                "EXISTS (SELECT 1 FROM discount_codes c
                     WHERE c.discount_id = discounts.id AND c.normalized = ${})",
                params.len()
            ));
        }
        if let Some(after) = &after {
            params.push((after, Type::UUID));
            conditions.push(format!("id < ${}", params.len()));
        }
        params.push((&fetch, Type::INT8));
        let filter_sql = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };
        let mut conn = self.conn().await;
        let found = conn
            .db()
            .query_typed(
                &format!(
                    "SELECT {} FROM discounts {filter_sql} ORDER BY id DESC LIMIT ${}",
                    columns(""),
                    params.len()
                ),
                &params,
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(discount_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Page::from_overfetch(found, page.limit))
    }

    async fn live_automatic(&self, at: Timestamp) -> Result<Vec<Discount>, RepoError> {
        let at = at.unix_micros();
        let mut conn = self.conn().await;
        conn.db()
            .query_typed(
                &format!(
                    "SELECT {} FROM discounts WHERE method = 'automatic' AND {} ORDER BY id",
                    columns(""),
                    phase_sql(Phase::Active, &ts(1))
                ),
                &[(&at, Type::INT8)],
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(discount_from_row)
            .collect()
    }

    async fn count_live_automatic(&self, at: Timestamp) -> Result<u64, RepoError> {
        let at = at.unix_micros();
        let mut conn = self.conn().await;
        let row = conn
            .db()
            .query_typed_one(
                &format!(
                    "SELECT count(*) AS n FROM discounts
                     WHERE method = 'automatic' AND status = 'active'
                         AND (ends_at IS NULL OR ends_at > {})",
                    ts(1)
                ),
                &[(&at, Type::INT8)],
            )
            .await
            .map_err(repo_error)?;
        let n: i64 = col(&row, "n")?;
        u64::try_from(n).map_err(corrupt)
    }

    async fn with_boundary_after(&self, after: Timestamp) -> Result<Vec<Discount>, RepoError> {
        let after = after.unix_micros();
        let mut conn = self.conn().await;
        conn.db()
            .query_typed(
                &format!(
                    "SELECT {} FROM discounts
                     WHERE status = 'active' AND (starts_at > {at} OR ends_at > {at})
                     ORDER BY id",
                    columns(""),
                    at = ts(1)
                ),
                &[(&after, Type::INT8)],
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(discount_from_row)
            .collect()
    }

    async fn schedule_ran_until(&self) -> Result<Option<Timestamp>, RepoError> {
        let mut conn = self.conn().await;
        let row = conn
            .db()
            .query_typed_opt(
                &format!("SELECT {} FROM discount_schedule", micros("ran_until")),
                &[],
            )
            .await
            .map_err(repo_error)?;
        row.map(|r| col::<i64>(&r, "ran_until").map(Timestamp::from_unix_micros))
            .transpose()
    }

    async fn insert(&self, discount: &mut Discount) -> Result<(), RepoError> {
        stamp_insert(discount, self.now());
        let s = discount.snapshot();
        let classes = discount.classes();
        let events = discount.take_events();
        let stamp = self.event_stamp();
        in_transaction!(self, |tx| {
            insert_discount(tx, &s, classes).await?;
            insert_events(tx, &events, Some(Version::NEW.next()), stamp).await
        })?;
        discount.set_version(Version::NEW.next());
        Ok(())
    }

    async fn update(&self, discount: &mut Discount) -> Result<(), RepoError> {
        stamp_update(discount, self.now());
        let s = discount.snapshot();
        let classes = discount.classes();
        let events = discount.take_events();
        let stamp = self.event_stamp();
        let version = in_transaction!(self, |tx| {
            let version = update_discount(tx, &s, classes).await?;
            insert_events(tx, &events, Some(version), stamp).await?;
            Ok(version)
        })?;
        discount.set_version(version);
        Ok(())
    }

    async fn delete(&self, discount: &mut Discount) -> Result<(), RepoError> {
        let id = discount.id().as_uuid();
        let version = version_to_db(discount.version())?;
        let events = discount.take_events();
        let stamp = self.event_stamp();
        in_transaction!(self, |tx| {
            // Codes, the counter and usages go with it.
            let deleted = tx
                .query_typed_opt(
                    "DELETE FROM discounts WHERE id = $1 AND version = $2 RETURNING id",
                    &[(&id, Type::UUID), (&version, Type::INT8)],
                )
                .await
                .map_err(repo_error)?;
            if deleted.is_none() {
                return Err(RepoError::StaleVersion);
            }
            insert_events(tx, &events, None, stamp).await
        })
    }

    async fn find_codes(&self, normalized: &[String]) -> Result<Vec<DiscountCode>, RepoError> {
        if normalized.is_empty() {
            return Ok(Vec::new());
        }
        let mut conn = self.conn().await;
        conn.db()
            .query_typed(
                &format!(
                    "SELECT {} FROM discount_codes WHERE normalized = ANY($1)",
                    code_columns()
                ),
                &[(&normalized, Type::TEXT_ARRAY)],
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(code_from_row)
            .collect()
    }

    async fn code_page(
        &self,
        discount: DiscountId,
        filter: &CodeFilter,
        page: PageRequest<String>,
    ) -> Result<Page<DiscountCode>, RepoError> {
        let id = discount.as_uuid();
        let fetch = overfetch(page.limit);
        let mut params: Vec<Param<'_>> = vec![(&id, Type::UUID)];
        let mut conditions = vec!["discount_id = $1".to_string()];
        code_conditions(filter, &mut params, &mut conditions);
        if let Some(after) = &page.after {
            params.push((after, Type::TEXT));
            conditions.push(format!("normalized > ${}", params.len()));
        }
        params.push((&fetch, Type::INT8));
        let mut conn = self.conn().await;
        let found = conn
            .db()
            .query_typed(
                &format!(
                    "SELECT {} FROM discount_codes WHERE {} ORDER BY normalized LIMIT ${}",
                    code_columns(),
                    conditions.join(" AND "),
                    params.len()
                ),
                &params,
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(code_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Page::from_overfetch(found, page.limit))
    }

    async fn count_codes(
        &self,
        discount: DiscountId,
        filter: &CodeFilter,
    ) -> Result<u64, RepoError> {
        let id = discount.as_uuid();
        let mut params: Vec<Param<'_>> = vec![(&id, Type::UUID)];
        let mut conditions = vec!["discount_id = $1".to_string()];
        code_conditions(filter, &mut params, &mut conditions);
        let mut conn = self.conn().await;
        let row = conn
            .db()
            .query_typed_one(
                &format!(
                    "SELECT count(*) AS n FROM discount_codes WHERE {}",
                    conditions.join(" AND ")
                ),
                &params,
            )
            .await
            .map_err(repo_error)?;
        let n: i64 = col(&row, "n")?;
        u64::try_from(n).map_err(corrupt)
    }

    async fn code_counts(
        &self,
        discounts: &[DiscountId],
    ) -> Result<HashMap<DiscountId, u64>, RepoError> {
        if discounts.is_empty() {
            return Ok(HashMap::new());
        }
        let ids = uuids(discounts, DiscountId::as_uuid);
        let mut conn = self.conn().await;
        conn.db()
            .query_typed(
                "SELECT discount_id, count(*) AS n FROM discount_codes
                 WHERE discount_id = ANY($1) GROUP BY discount_id",
                &[(&ids, Type::UUID_ARRAY)],
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(|row| {
                let n: i64 = col(row, "n")?;
                Ok((
                    DiscountId::from_uuid(col(row, "discount_id")?),
                    u64::try_from(n).map_err(corrupt)?,
                ))
            })
            .collect()
    }

    async fn add_codes(&self, codes: &[DiscountCode]) -> Result<(), RepoError> {
        let codes = codes.to_vec();
        in_transaction!(self, |tx| add_codes(tx, &codes).await)
    }

    async fn usage_counts(
        &self,
        discounts: &[DiscountId],
    ) -> Result<HashMap<DiscountId, u64>, RepoError> {
        if discounts.is_empty() {
            return Ok(HashMap::new());
        }
        let ids = uuids(discounts, DiscountId::as_uuid);
        let mut conn = self.conn().await;
        conn.db()
            .query_typed(
                "SELECT discount_id, used FROM discount_usage_counts
                 WHERE discount_id = ANY($1) AND used > 0",
                &[(&ids, Type::UUID_ARRAY)],
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(|row| {
                let n: i64 = col(row, "used")?;
                Ok((
                    DiscountId::from_uuid(col(row, "discount_id")?),
                    u64::try_from(n).map_err(corrupt)?,
                ))
            })
            .collect()
    }

    async fn discounted_totals(
        &self,
        discounts: &[DiscountId],
    ) -> Result<HashMap<DiscountId, Vec<Money>>, RepoError> {
        if discounts.is_empty() {
            return Ok(HashMap::new());
        }
        let ids = uuids(discounts, DiscountId::as_uuid);
        let mut conn = self.conn().await;
        let rows = conn
            .db()
            .query_typed(
                "SELECT discount_id, currency, sum(amount)::bigint AS total FROM discount_usages
                 WHERE discount_id = ANY($1) GROUP BY discount_id, currency
                 ORDER BY discount_id, currency",
                &[(&ids, Type::UUID_ARRAY)],
            )
            .await
            .map_err(repo_error)?;
        let mut totals: HashMap<DiscountId, Vec<Money>> = HashMap::new();
        for row in &rows {
            let currency: String = col(row, "currency")?;
            let total =
                Money::new(col(row, "total")?, currency_from_db(&currency)?).map_err(corrupt)?;
            totals
                .entry(DiscountId::from_uuid(col(row, "discount_id")?))
                .or_default()
                .push(total);
        }
        Ok(totals)
    }

    async fn used_by(
        &self,
        discounts: &[DiscountId],
        customer: CustomerId,
    ) -> Result<HashSet<DiscountId>, RepoError> {
        if discounts.is_empty() {
            return Ok(HashSet::new());
        }
        let ids = uuids(discounts, DiscountId::as_uuid);
        let mut conn = self.conn().await;
        conn.db()
            .query_typed(
                "SELECT DISTINCT discount_id FROM discount_usages
                 WHERE discount_id = ANY($1) AND customer_id = $2",
                &[(&ids, Type::UUID_ARRAY), (&customer.as_uuid(), Type::UUID)],
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(|row| Ok(DiscountId::from_uuid(col(row, "discount_id")?)))
            .collect()
    }

    async fn usage_page(
        &self,
        discount: DiscountId,
        page: PageRequest<OrderId>,
    ) -> Result<Page<Usage>, RepoError> {
        let id = discount.as_uuid();
        let after = page.after.map(|o| o.as_uuid());
        let fetch = overfetch(page.limit);
        let mut conn = self.conn().await;
        let found = conn
            .db()
            .query_typed(
                &format!(
                    "SELECT discount_id, order_id, customer_id, code, amount, currency, {}
                     FROM discount_usages
                     WHERE discount_id = $1 AND ($2::uuid IS NULL OR order_id < $2)
                     ORDER BY order_id DESC LIMIT $3",
                    micros("used_at")
                ),
                &[
                    (&id, Type::UUID),
                    (&after, Type::UUID),
                    (&fetch, Type::INT8),
                ],
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(|row| {
                Ok(Usage {
                    discount: DiscountId::from_uuid(col(row, "discount_id")?),
                    order: OrderId::from_uuid(col(row, "order_id")?),
                    customer: CustomerId::from_uuid(col(row, "customer_id")?),
                    code: col(row, "code")?,
                    amount: Money::new(
                        col(row, "amount")?,
                        currency_from_db(&col::<String>(row, "currency")?)?,
                    )
                    .map_err(corrupt)?,
                    at: Timestamp::from_unix_micros(col(row, "used_at")?),
                })
            })
            .collect::<Result<Vec<_>, RepoError>>()?;
        Ok(Page::from_overfetch(found, page.limit))
    }
}

fn code_conditions<'a>(
    filter: &'a CodeFilter,
    params: &mut Vec<Param<'a>>,
    conditions: &mut Vec<String>,
) {
    if let Some(q) = &filter.q {
        params.push((q, Type::TEXT));
        conditions.push(format!("strpos(normalized, ${}) > 0", params.len()));
    }
    if let Some(batch) = &filter.batch {
        params.push((batch, Type::TEXT));
        conditions.push(format!("batch = ${}", params.len()));
    }
    if let Some(active) = &filter.active {
        params.push((active, Type::BOOL));
        conditions.push(format!("active = ${}", params.len()));
    }
}

/// Moves the schedule job's progress from `from` (`None`: no row yet) to `to`, or fails
/// with `StaleVersion` when another run moved it first.
pub(crate) async fn advance_discount_schedule(
    client: &impl Queries,
    from: Option<Timestamp>,
    to: Timestamp,
) -> Result<(), RepoError> {
    let moved = match from {
        None => client
            .query_typed(
                &format!(
                    "INSERT INTO discount_schedule (ran_until) VALUES ({})
                     ON CONFLICT (id) DO NOTHING RETURNING id",
                    ts(1)
                ),
                &[(&to.unix_micros(), Type::INT8)],
            )
            .await
            .map_err(repo_error)?,
        Some(from) => client
            .query_typed(
                &format!(
                    "UPDATE discount_schedule SET ran_until = {} WHERE ran_until = {}
                     RETURNING id",
                    ts(1),
                    ts(2)
                ),
                &[
                    (&to.unix_micros(), Type::INT8),
                    (&from.unix_micros(), Type::INT8),
                ],
            )
            .await
            .map_err(repo_error)?,
    };
    if moved.is_empty() {
        return Err(RepoError::StaleVersion);
    }
    Ok(())
}
