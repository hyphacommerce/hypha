//! What a cart's discounts are weighed from, loaded in a fixed number of reads, and the
//! promotions a product page shows.

use std::collections::{HashMap, HashSet};

use futures_util::future::try_join;

use hypha_core::{
    CategoryId, ChannelId, CustomerId, DiscountId, MarketId, ProductId, RepoError, Timestamp,
};

use crate::{
    code::DiscountCode,
    discount::{Condition, Discount, Kind, Method, Target},
    engine::Live,
    repo::{DiscountRepo, HasDiscounts},
    summary::{Audience, Names, summary},
};

/// The live automatic discounts, the cart's codes with their discounts, and the counts
/// their limits are judged by.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiscountContext {
    pub automatic: Vec<Discount>,
    /// The cart's codes that exist and are active, each with its discount.
    pub coded: Vec<(DiscountCode, Discount)>,
    /// Uses so far of the discounts above that have a total limit.
    pub used: HashMap<DiscountId, u64>,
    /// The once-per-customer ones above the buyer used.
    pub used_by_buyer: HashSet<DiscountId>,
}

impl DiscountContext {
    /// Loads the discounts a cart holding `codes` (normalised) may get at `at`: the live
    /// automatic ones and the codes' (in the order given), the two reads sent together.
    /// Their usage comes after (`usage`), once the buyer is known.
    pub async fn load<S: HasDiscounts>(
        ports: &S,
        at: Timestamp,
        codes: &[String],
    ) -> Result<Self, RepoError> {
        let repo = ports.discounts();
        let coded = async {
            if codes.is_empty() {
                return Ok(Vec::new());
            }
            let mut found: Vec<DiscountCode> = repo
                .find_codes(codes)
                .await?
                .into_iter()
                .filter(|c| c.active)
                .collect();
            found.sort_by_key(|c| codes.iter().position(|e| *e == c.normalized));
            let ids: Vec<DiscountId> = found.iter().map(|c| c.discount).collect();
            let discounts = if ids.is_empty() {
                Vec::new()
            } else {
                repo.get_many(&ids).await?
            };
            Ok(found
                .into_iter()
                .filter_map(|code| {
                    let discount = discounts.iter().find(|d| d.id() == code.discount)?;
                    (discount.method() == Method::Codes).then(|| (code, discount.clone()))
                })
                .collect())
        };
        let (automatic, coded) = try_join(repo.live_automatic(at), coded).await?;
        Ok(Self {
            automatic,
            coded,
            used: HashMap::new(),
            used_by_buyer: HashSet::new(),
        })
    }

    /// Whether a discount here is once per customer: only then does the buyer matter to
    /// its limits.
    pub fn counts_per_customer(&self) -> bool {
        self.all().any(|d| d.limits().once_per_customer)
    }

    /// The uses so far of the discounts here with a total limit, and which of the once
    /// per customer ones `customer` used, the two reads sent together; `set_usage` keeps
    /// them.
    pub async fn usage<S: HasDiscounts>(
        &self,
        ports: &S,
        customer: Option<CustomerId>,
    ) -> Result<(HashMap<DiscountId, u64>, HashSet<DiscountId>), RepoError> {
        let repo = ports.discounts();
        let limited: Vec<DiscountId> = self
            .all()
            .filter(|d| d.limits().total.is_some())
            .map(|d| d.id())
            .collect();
        let once: Vec<DiscountId> = self
            .all()
            .filter(|d| d.limits().once_per_customer)
            .map(|d| d.id())
            .collect();
        let used = async {
            if limited.is_empty() {
                Ok(HashMap::new())
            } else {
                repo.usage_counts(&limited).await
            }
        };
        let used_by_buyer = async {
            match customer {
                Some(customer) if !once.is_empty() => repo.used_by(&once, customer).await,
                _ => Ok(HashSet::new()),
            }
        };
        try_join(used, used_by_buyer).await
    }

    pub fn set_usage(
        &mut self,
        (used, used_by_buyer): (HashMap<DiscountId, u64>, HashSet<DiscountId>),
    ) {
        self.used = used;
        self.used_by_buyer = used_by_buyer;
    }

    fn all(&self) -> impl Iterator<Item = &Discount> {
        self.automatic
            .iter()
            .chain(self.coded.iter().map(|(_, d)| d))
    }

    /// Whether a discount here names categories: only then does pricing need the
    /// products' categories.
    pub fn names_categories(&self) -> bool {
        let names = |set: &crate::discount::ItemSet| {
            matches!(set.items, crate::discount::Items::Categories(_))
        };
        self.automatic
            .iter()
            .chain(self.coded.iter().map(|(_, d)| d))
            .any(|d| match d.kind() {
                Kind::Standard { rewards } => rewards.iter().any(|r| match &r.target {
                    Target::Items(set) => names(set),
                    _ => false,
                }),
                Kind::BuyXGetY { buys, gets, .. } => names(buys) || names(gets),
                Kind::Tiered { items, .. } => names(items),
            })
    }

    /// Every discount that may apply, each with whether its limits are reached.
    pub fn candidates(&self) -> Vec<Live<'_>> {
        let reached = |d: &Discount| {
            let total = d
                .limits()
                .total
                .is_some_and(|max| self.used.get(&d.id()).copied().unwrap_or(0) >= u64::from(max));
            total || (d.limits().once_per_customer && self.used_by_buyer.contains(&d.id()))
        };
        self.automatic
            .iter()
            .map(|d| Live {
                discount: d,
                code: None,
                limit_reached: reached(d),
            })
            .chain(self.coded.iter().map(|(code, d)| Live {
                discount: d,
                code: Some(code.normalized.as_str()),
                limit_reached: reached(d) || !code.has_uses_left(),
            }))
            .collect()
    }

    /// The discount and code row for an applied discount: what checkout claims uses of.
    pub fn find(&self, discount: DiscountId) -> Option<&Discount> {
        self.automatic
            .iter()
            .chain(self.coded.iter().map(|(_, d)| d))
            .find(|d| d.id() == discount)
    }

    pub fn code(&self, normalized: &str) -> Option<&DiscountCode> {
        self.coded
            .iter()
            .map(|(c, _)| c)
            .find(|c| c.normalized == normalized)
    }
}

/// A promotion on a product page: a live automatic discount that may reach the product
/// and whose conditions are only about market and channel, so every shopper there sees
/// it alike. A badge, never a price.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Promotion {
    pub discount: DiscountId,
    pub title: String,
    pub summary: String,
}

/// The promotions of `product` (in `categories` with their ancestors) among `automatic`
/// (live ones, `DiscountRepo::live_automatic`), in `market` and `channel` at `at`, by ID,
/// summarised with `names` (`NameRequest::for_shoppers`).
pub fn promotions(
    automatic: &[Discount],
    product: ProductId,
    categories: &HashSet<CategoryId>,
    market: MarketId,
    channel: Option<ChannelId>,
    at: Timestamp,
    names: &Names,
) -> Vec<Promotion> {
    automatic
        .iter()
        .filter(|d| d.method() == Method::Automatic && d.is_live(at))
        .filter(|d| {
            d.conditions().iter().all(|c| match c {
                Condition::Markets(ids) => ids.contains(&market),
                Condition::Channels(ids) => channel.is_some_and(|ch| ids.contains(&ch)),
                // Item minimums are fine on a badge; conditions about the buyer aren't.
                Condition::MinSubtotal(_) | Condition::MinQuantity(_) => true,
                Condition::CustomerGroups(_) | Condition::Customers(_) | Condition::SignedIn => {
                    false
                }
            })
        })
        .filter(|d| reaches(d.kind(), product, categories))
        .map(|d| Promotion {
            discount: d.id(),
            title: d.title().to_string(),
            summary: summary(d, Some(market), names, Audience::Shoppers),
        })
        .collect()
}

fn reaches(kind: &Kind, product: ProductId, categories: &HashSet<CategoryId>) -> bool {
    match kind {
        Kind::Standard { rewards } => rewards.iter().any(|r| match &r.target {
            Target::Items(set) => set.may_reach(product, categories),
            Target::Subtotal | Target::Shipping { .. } => false,
        }),
        Kind::BuyXGetY { buys, gets, .. } => {
            buys.may_reach(product, categories) || gets.may_reach(product, categories)
        }
        Kind::Tiered { items, .. } => items.may_reach(product, categories),
    }
}
