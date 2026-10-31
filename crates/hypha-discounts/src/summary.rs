//! A discount in one sentence, built from its data: what the admin, emails, the storefront
//! and agents all show, so they say the same.
//!
//! The discount holds only IDs, so the caller hands in the names it found for them
//! (`Names`, looked up in one read with `DiscountRepo::names`); whatever has no name is
//! counted instead ("2 categories"). Amounts are shown in `market` when it has one, else
//! every market's, and dates in UTC.

use std::collections::BTreeMap;

use hypha_core::{
    BasisPoints, CategoryId, ChannelId, CustomerGroupId, CustomerId, MarketId, Money, ProductId,
    Timestamp, VariantId,
};

use crate::discount::{
    Allocation, Condition, Discount, ItemSet, Items, Kind, PerMarket, Target, Threshold, Value,
};

/// What the IDs a discount holds are called. A missing name is counted instead.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Names {
    /// Product titles.
    pub products: BTreeMap<ProductId, String>,
    /// The product's title with the variant's option values: "Grow Kit (Oyster / Large)".
    pub variants: BTreeMap<VariantId, String>,
    pub categories: BTreeMap<CategoryId, String>,
    pub markets: BTreeMap<MarketId, String>,
    pub channels: BTreeMap<ChannelId, String>,
    pub customer_groups: BTreeMap<CustomerGroupId, String>,
    /// First and last name; never an email address.
    pub customers: BTreeMap<CustomerId, String>,
}

static NO_NAMES: Names = Names::new();

impl Names {
    pub const fn new() -> Self {
        Self {
            products: BTreeMap::new(),
            variants: BTreeMap::new(),
            categories: BTreeMap::new(),
            markets: BTreeMap::new(),
            channels: BTreeMap::new(),
            customer_groups: BTreeMap::new(),
            customers: BTreeMap::new(),
        }
    }

    /// No names at all: everything is counted.
    pub fn none() -> &'static Names {
        &NO_NAMES
    }
}

/// The IDs whose names some discounts' summaries need: what `DiscountRepo::names` reads.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NameRequest {
    pub products: Vec<ProductId>,
    pub variants: Vec<VariantId>,
    pub categories: Vec<CategoryId>,
    pub markets: Vec<MarketId>,
    pub channels: Vec<ChannelId>,
    pub customer_groups: Vec<CustomerGroupId>,
    pub customers: Vec<CustomerId>,
    /// Name only active products (and their variants): a shopper never sees a draft's
    /// title.
    pub active_products_only: bool,
}

impl NameRequest {
    /// Every ID the discounts name, for staff.
    pub fn for_staff<'a>(discounts: impl IntoIterator<Item = &'a Discount>) -> Self {
        let mut request = Self::default();
        for d in discounts {
            request.add(d);
        }
        request.tidy();
        request
    }

    /// What a shopper's summary names: never customers or groups (`Audience::Shoppers`
    /// doesn't name them), and only active products.
    pub fn for_shoppers<'a>(discounts: impl IntoIterator<Item = &'a Discount>) -> Self {
        let mut request = Self::for_staff(discounts);
        request.customer_groups.clear();
        request.customers.clear();
        request.active_products_only = true;
        request
    }

    pub fn is_empty(&self) -> bool {
        self.products.is_empty()
            && self.variants.is_empty()
            && self.categories.is_empty()
            && self.markets.is_empty()
            && self.channels.is_empty()
            && self.customer_groups.is_empty()
            && self.customers.is_empty()
    }

    fn add(&mut self, d: &Discount) {
        let mut set = |s: &ItemSet| match &s.items {
            Items::All => {}
            Items::Products(ids) => self.products.extend(ids),
            Items::Variants(ids) => self.variants.extend(ids),
            Items::Categories(ids) => self.categories.extend(ids),
        };
        match d.kind() {
            Kind::Standard { rewards } => {
                for r in rewards {
                    if let Target::Items(s) = &r.target {
                        set(s);
                    }
                }
            }
            Kind::BuyXGetY { buys, gets, .. } => {
                set(buys);
                set(gets);
            }
            Kind::Tiered { items, .. } => set(items),
        }
        for c in d.conditions() {
            match c {
                Condition::Markets(ids) => self.markets.extend(ids),
                Condition::Channels(ids) => self.channels.extend(ids),
                Condition::CustomerGroups(ids) => self.customer_groups.extend(ids),
                Condition::Customers(ids) => self.customers.extend(ids),
                Condition::SignedIn | Condition::MinSubtotal(_) | Condition::MinQuantity(_) => {}
            }
        }
    }

    fn tidy(&mut self) {
        fn tidy<T: Ord>(v: &mut Vec<T>) {
            v.sort();
            v.dedup();
        }
        tidy(&mut self.products);
        tidy(&mut self.variants);
        tidy(&mut self.categories);
        tidy(&mut self.markets);
        tidy(&mut self.channels);
        tidy(&mut self.customer_groups);
        tidy(&mut self.customers);
    }
}

/// Who reads the sentence. Shoppers never see who else a discount is for: groups read
/// "for members" and customers "for selected customers".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Audience {
    Staff,
    Shoppers,
}

/// "10% off Grow kits on orders from €50 · until 31 Oct 2026".
pub fn summary(
    discount: &Discount,
    market: Option<MarketId>,
    names: &Names,
    audience: Audience,
) -> String {
    let say = Say {
        market,
        names,
        audience,
    };
    let mut sentence = say.kind(discount.kind());
    let mut conditions: Vec<&Condition> = discount.conditions().iter().collect();
    conditions.sort_by_key(|c| rank(c));
    for condition in conditions {
        sentence.push(' ');
        sentence.push_str(&say.condition(condition));
    }
    let mut parts = vec![sentence];
    let limits = discount.limits();
    if let Some(total) = limits.total {
        parts.push(plural(u64::from(total), "use", "uses"));
    }
    if limits.once_per_customer {
        parts.push("once per customer".to_string());
    }
    if let Some(start) = discount.starts_at() {
        parts.push(format!("from {}", date(start)));
    }
    if let Some(end) = discount.ends_at() {
        // The end is exclusive: a discount ending at midnight runs until the day before.
        parts.push(format!(
            "until {}",
            date(Timestamp::from_unix_micros(
                end.unix_micros().saturating_sub(1)
            ))
        ));
    }
    parts.join(" · ")
}

/// Where a condition goes in the sentence: where, then who, then how much.
fn rank(c: &Condition) -> u8 {
    match c {
        Condition::Markets(_) => 0,
        Condition::Channels(_) => 1,
        Condition::SignedIn => 2,
        Condition::CustomerGroups(_) => 3,
        Condition::Customers(_) => 4,
        Condition::MinSubtotal(_) => 5,
        Condition::MinQuantity(_) => 6,
    }
}

struct Say<'a> {
    market: Option<MarketId>,
    names: &'a Names,
    audience: Audience,
}

/// What an item set names, as the sentence can put it.
enum Subject {
    All,
    /// Products or variants, by name ("Liquid Culture Syringe", "A and 2 more") with
    /// whether it's a single one; or counted ("2 products").
    Products {
        text: String,
        single: bool,
    },
    /// Categories by name, or counted ("2 categories").
    Categories(String),
}

impl Say<'_> {
    fn kind(&self, kind: &Kind) -> String {
        match kind {
            Kind::Standard { rewards } => rewards
                .iter()
                .map(|r| self.reward(&r.target, &r.value))
                .collect::<Vec<_>>()
                .join(" + "),
            Kind::BuyXGetY {
                buys,
                buy,
                gets,
                get_quantity,
                value,
                uses_per_order,
            } => {
                let buy = match buy {
                    Threshold::Quantity(n) => format!("Buy {}", self.counted(*n, buys)),
                    Threshold::Amount(amounts) => {
                        let spend = format!("Spend {}", self.amounts(amounts));
                        match self.subject(buys) {
                            Subject::All => format!("{spend}{}", not_on_sale(buys)),
                            _ => format!("{spend} on {}", self.noun(buys)),
                        }
                    }
                };
                let got = match value {
                    Value::Percent(bp) if bp.value() == 10_000 => "free".to_string(),
                    Value::Percent(bp) => format!("at {} off", percent(*bp)),
                    Value::Amount { amounts, .. } | Value::FixedPrice(amounts) => {
                        let each = if *get_quantity > 1 { " each" } else { "" };
                        format!("at {} off{each}", self.amounts(amounts))
                    }
                };
                let mut phrase = format!("{buy}, get {} {got}", self.counted(*get_quantity, gets));
                match uses_per_order {
                    Some(1) => phrase.push_str(" (once per order)"),
                    Some(times) => phrase.push_str(&format!(" (up to {times} times per order)")),
                    None => {}
                }
                phrase
            }
            Kind::Tiered { items, tiers, .. } => {
                let steps: Vec<String> = tiers
                    .iter()
                    .map(|t| {
                        let from = match &t.threshold {
                            Threshold::Quantity(n) => format!("{n}+ items"),
                            Threshold::Amount(amounts) => format!("{}+", self.amounts(amounts)),
                        };
                        format!("{} {from}", self.value_off(&t.value))
                    })
                    .collect();
                format!("{}{}", steps.join(", "), self.within(items))
            }
        }
    }

    fn reward(&self, target: &Target, value: &Value) -> String {
        match target {
            Target::Items(set) => match value {
                Value::Percent(bp) => format!("{} off {}", percent(*bp), self.noun(set)),
                Value::Amount {
                    amounts,
                    allocation,
                } => {
                    let amount = self.amounts(amounts);
                    match allocation {
                        Allocation::Each => format!("{amount} off {}", self.each(set)),
                        Allocation::Across => format!("{amount} off {}", self.noun(set)),
                        Allocation::Cheapest { units } => format!(
                            "{amount} off the cheapest {}{}",
                            plural(u64::from(*units), "item", "items"),
                            self.within(set)
                        ),
                    }
                }
                Value::FixedPrice(amounts) => format!(
                    "{} at {} each",
                    capitalized(&self.noun(set)),
                    self.amounts(amounts)
                ),
            },
            Target::Subtotal => format!("{} the order", self.value_off(value)),
            Target::Shipping { max_amount } => {
                let mut phrase = match value {
                    Value::Percent(bp) if bp.value() == 10_000 => "Free shipping".to_string(),
                    _ => format!("{} shipping", self.value_off(value)),
                };
                if let Some(max) = max_amount {
                    phrase.push_str(&format!(" up to {}", self.amounts(max)));
                }
                phrase
            }
        }
    }

    /// "10% off", "€5 off".
    fn value_off(&self, value: &Value) -> String {
        match value {
            Value::Percent(bp) => format!("{} off", percent(*bp)),
            Value::Amount { amounts, .. } | Value::FixedPrice(amounts) => {
                format!("{} off", self.amounts(amounts))
            }
        }
    }

    fn condition(&self, condition: &Condition) -> String {
        match condition {
            Condition::Markets(ids) => format!(
                "in {}",
                named(ids, &self.names.markets).unwrap_or_else(|| plural(
                    ids.len() as u64,
                    "market",
                    "markets"
                ))
            ),
            Condition::Channels(ids) => format!(
                "in {}",
                named(ids, &self.names.channels).unwrap_or_else(|| plural(
                    ids.len() as u64,
                    "channel",
                    "channels"
                ))
            ),
            Condition::CustomerGroups(ids) => match self.audience {
                Audience::Shoppers => "for members".to_string(),
                Audience::Staff => format!(
                    "for {}",
                    named(ids, &self.names.customer_groups).unwrap_or_else(|| plural(
                        ids.len() as u64,
                        "customer group",
                        "customer groups"
                    ))
                ),
            },
            Condition::Customers(ids) => match self.audience {
                Audience::Shoppers => "for selected customers".to_string(),
                Audience::Staff => format!(
                    "for {}",
                    named(ids, &self.names.customers).unwrap_or_else(|| plural(
                        ids.len() as u64,
                        "customer",
                        "customers"
                    ))
                ),
            },
            Condition::SignedIn => "for signed-in customers".to_string(),
            Condition::MinSubtotal(amounts) => {
                format!("on orders from {}", self.amounts(amounts))
            }
            Condition::MinQuantity(n) => format!("with {n}+ items"),
        }
    }

    fn subject(&self, set: &ItemSet) -> Subject {
        match &set.items {
            Items::All => Subject::All,
            Items::Products(ids) => Subject::Products {
                text: named(ids, &self.names.products)
                    .unwrap_or_else(|| plural(ids.len() as u64, "product", "products")),
                single: ids.len() == 1 && self.names.products.contains_key(&ids[0]),
            },
            Items::Variants(ids) => Subject::Products {
                text: named(ids, &self.names.variants)
                    .unwrap_or_else(|| plural(ids.len() as u64, "variant", "variants")),
                single: ids.len() == 1 && self.names.variants.contains_key(&ids[0]),
            },
            Items::Categories(ids) => Subject::Categories(
                named(ids, &self.names.categories)
                    .unwrap_or_else(|| plural(ids.len() as u64, "category", "categories")),
            ),
        }
    }

    /// "all items", "Liquid Culture Syringe", "Grow kits and Cultures".
    fn noun(&self, set: &ItemSet) -> String {
        let text = match self.subject(set) {
            Subject::All => "all items".to_string(),
            Subject::Products { text, .. } | Subject::Categories(text) => text,
        };
        format!("{text}{}", not_on_sale(set))
    }

    /// "each item", "each item in Grow kits", "each Liquid Culture Syringe", "each of A and B".
    fn each(&self, set: &ItemSet) -> String {
        let text = match self.subject(set) {
            Subject::All => "each item".to_string(),
            Subject::Categories(text) => format!("each item in {text}"),
            Subject::Products { text, single: true } => format!("each {text}"),
            Subject::Products { text, .. } => format!("each of {text}"),
        };
        format!("{text}{}", not_on_sale(set))
    }

    /// "2 items", "1 from Grow kits", "1 Liquid Culture Syringe", "1 of A and B".
    fn counted(&self, n: u32, set: &ItemSet) -> String {
        let text = match self.subject(set) {
            Subject::All => plural(u64::from(n), "item", "items"),
            Subject::Categories(text) => format!("{n} from {text}"),
            Subject::Products { text, single: true } => format!("{n} {text}"),
            Subject::Products { text, .. } => format!("{n} of {text}"),
        };
        format!("{text}{}", not_on_sale(set))
    }

    /// What follows a count of items: "", " in Cultures", " of Liquid Culture Syringe".
    fn within(&self, set: &ItemSet) -> String {
        let text = match self.subject(set) {
            Subject::All => String::new(),
            Subject::Categories(text) => format!(" in {text}"),
            Subject::Products { text, .. } => format!(" of {text}"),
        };
        format!("{text}{}", not_on_sale(set))
    }

    /// The market's amount when it has one, else every market's: "€5 / 600 JPY".
    fn amounts(&self, amounts: &PerMarket) -> String {
        if let Some(amount) = self.market.and_then(|m| amounts.get(m)) {
            return money(amount);
        }
        amounts
            .iter()
            .map(|(_, a)| money(a))
            .collect::<Vec<_>>()
            .join(" / ")
    }
}

fn not_on_sale(set: &ItemSet) -> &'static str {
    if set.exclude_on_sale {
        " not on sale"
    } else {
        ""
    }
}

/// The names of `ids`, short: "A", "A and B", "A and 2 more"; those without a name are
/// in the "more". `None` when none has a name, for the caller to count them.
fn named<K: Ord>(ids: &[K], names: &BTreeMap<K, String>) -> Option<String> {
    let known: Vec<&str> = ids
        .iter()
        .filter_map(|id| names.get(id).map(String::as_str))
        .collect();
    let first = *known.first()?;
    match (ids.len(), known.get(1)) {
        (1, _) => Some(first.to_string()),
        (2, Some(second)) => Some(format!("{first} and {second}")),
        (n, _) => Some(format!("{first} and {} more", n - 1)),
    }
}

fn capitalized(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn plural(n: u64, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

/// 1000 → "10%", 1250 → "12.5%", 1234 → "12.34%".
pub(crate) fn percent(bp: BasisPoints) -> String {
    let v = bp.value();
    let (whole, frac) = (v / 100, (v % 100).abs());
    match frac {
        0 => format!("{whole}%"),
        f if f % 10 == 0 => format!("{whole}.{}%", f / 10),
        f => format!("{whole}.{f:02}%"),
    }
}

/// The symbol written before the amount for the currencies whose symbol is unambiguous;
/// every other currency is written by its code after the amount (as emails do).
fn symbol(code: &str) -> Option<&'static str> {
    match code {
        "EUR" => Some("€"),
        "USD" => Some("$"),
        "GBP" => Some("£"),
        _ => None,
    }
}

/// "€12.50", "€50", "6,000 RSD", "1,800 JPY": thousands grouped with `,`, the currency's
/// minor units after a `.` unless they're all zero.
pub fn money(m: Money) -> String {
    let currency = m.currency();
    let digits = u32::from(currency.minor_units());
    let abs = m.amount().unsigned_abs();
    let scale = 10u64.pow(digits);
    let (whole, fraction) = (abs / scale, abs % scale);
    let mut number = group_thousands(whole);
    if fraction != 0 {
        number.push_str(&format!(".{fraction:0width$}", width = digits as usize));
    }
    let sign = if m.amount() < 0 { "-" } else { "" };
    match symbol(currency.as_str()) {
        Some(symbol) => format!("{sign}{symbol}{number}"),
        None => format!("{sign}{number} {currency}"),
    }
}

fn group_thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// "31 Oct 2026", the UTC day of `at`.
fn date(at: Timestamp) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let days = at.unix_micros().div_euclid(86_400_000_000);
    let (y, m, d) = civil_from_days(days);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a civil month is 1 to 12"
    )]
    let month = MONTHS[(m - 1) as usize];
    format!("{d} {month} {y}")
}

/// The proleptic Gregorian date `days` after 1970-01-01 (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use hypha_core::{Currency, DiscountId};
    use uuid::Uuid;

    use super::*;
    use crate::discount::{ClassSet, DiscountDraft, Limits, Measure, Method, Reward};

    fn mkt(n: u128) -> MarketId {
        MarketId::from_uuid(Uuid::from_u128(n))
    }

    fn cat(n: u128) -> CategoryId {
        CategoryId::from_uuid(Uuid::from_u128(n))
    }

    fn prod(n: u128) -> ProductId {
        ProductId::from_uuid(Uuid::from_u128(n))
    }

    fn eur(n: i64) -> Money {
        Money::new(n, Currency::EUR).unwrap()
    }

    fn of_currency(n: i64, code: &str) -> Money {
        Money::new(n, Currency::new(code).unwrap()).unwrap()
    }

    fn bp(n: i32) -> BasisPoints {
        BasisPoints::new(n).unwrap()
    }

    fn discount(kind: Kind) -> DiscountDraft {
        DiscountDraft::new("Offer", Method::Automatic, kind)
    }

    fn built(draft: DiscountDraft) -> Discount {
        Discount::new(DiscountId::from_uuid(Uuid::from_u128(1)), draft).unwrap()
    }

    /// The store's names: categories 1 to 3, products 1 and 2, market 1.
    fn names() -> Names {
        let mut n = Names::new();
        n.categories.insert(cat(1), "Grow kits".into());
        n.categories.insert(cat(2), "Cultures".into());
        n.categories.insert(cat(3), "Lab gear".into());
        n.products.insert(prod(1), "Liquid Culture Syringe".into());
        n.products.insert(prod(2), "Spore Print".into());
        n.markets.insert(mkt(1), "Serbia".into());
        n
    }

    fn of(draft: DiscountDraft) -> String {
        summary(&built(draft), Some(mkt(1)), &names(), Audience::Staff)
    }

    fn for_shoppers(draft: DiscountDraft) -> String {
        summary(&built(draft), Some(mkt(1)), &names(), Audience::Shoppers)
    }

    fn standard(target: Target, value: Value) -> Kind {
        Kind::Standard {
            rewards: vec![Reward { target, value }],
        }
    }

    fn categories(ids: &[u128]) -> ItemSet {
        ItemSet {
            items: Items::Categories(ids.iter().map(|n| cat(*n)).collect()),
            exclude_on_sale: false,
        }
    }

    fn percent_off(set: ItemSet) -> DiscountDraft {
        discount(standard(Target::Items(set), Value::Percent(bp(1000))))
    }

    #[test]
    fn a_category_is_named() {
        assert_eq!(of(percent_off(categories(&[1]))), "10% off Grow kits");
    }

    #[test]
    fn two_names_are_both_said() {
        assert_eq!(
            of(percent_off(categories(&[1, 2]))),
            "10% off Grow kits and Cultures"
        );
    }

    #[test]
    fn more_than_two_names_say_the_first_and_how_many_more() {
        assert_eq!(
            of(percent_off(categories(&[1, 2, 3]))),
            "10% off Grow kits and 2 more"
        );
    }

    #[test]
    fn what_has_no_name_counts_among_the_more() {
        assert_eq!(
            of(percent_off(categories(&[9, 1]))),
            "10% off Grow kits and 1 more"
        );
    }

    #[test]
    fn without_any_name_they_are_counted() {
        assert_eq!(of(percent_off(categories(&[8, 9]))), "10% off 2 categories");
    }

    #[test]
    fn a_minimum_and_an_end_follow_the_reward() {
        let mut draft = percent_off(categories(&[1]));
        draft.conditions = vec![Condition::MinSubtotal(PerMarket::of(mkt(1), eur(5000)))];
        // Midnight on 1 Nov: the last day is 31 Oct.
        draft.ends_at = Some(Timestamp::from_unix_micros(1_793_491_200_000_000));
        assert_eq!(
            of(draft),
            "10% off Grow kits on orders from €50 · until 31 Oct 2026"
        );
    }

    #[test]
    fn free_shipping_in_a_market_over_a_minimum() {
        let mut draft = discount(standard(
            Target::Shipping { max_amount: None },
            Value::Percent(bp(10_000)),
        ));
        // Listed minimum first: the sentence still says where before how much.
        draft.conditions = vec![
            Condition::MinSubtotal(PerMarket::of(mkt(1), of_currency(600_000, "RSD"))),
            Condition::Markets(vec![mkt(1)]),
        ];
        assert_eq!(
            of(draft),
            "Free shipping in Serbia on orders from 6,000 RSD"
        );
    }

    #[test]
    fn an_amount_off_the_order_shows_the_markets_amount() {
        let draft = discount(standard(
            Target::Subtotal,
            Value::Amount {
                amounts: PerMarket::new([(mkt(1), eur(1500)), (mkt(2), eur(1600))].into()),
                allocation: Allocation::Across,
            },
        ));
        assert_eq!(of(draft), "€15 off the order");
    }

    #[test]
    fn without_a_market_every_markets_amount_is_shown() {
        let draft = discount(standard(
            Target::Subtotal,
            Value::Amount {
                amounts: PerMarket::new(
                    [(mkt(1), eur(1550)), (mkt(2), of_currency(1800, "JPY"))].into(),
                ),
                allocation: Allocation::Across,
            },
        ));
        assert_eq!(
            summary(&built(draft), None, &names(), Audience::Staff),
            "€15.50 / 1,800 JPY off the order"
        );
    }

    #[test]
    fn an_amount_off_each_product() {
        let draft = discount(standard(
            Target::Items(ItemSet {
                items: Items::Products(vec![prod(1)]),
                exclude_on_sale: false,
            }),
            Value::Amount {
                amounts: PerMarket::of(mkt(1), eur(500)),
                allocation: Allocation::Each,
            },
        ));
        assert_eq!(of(draft), "€5 off each Liquid Culture Syringe");
    }

    #[test]
    fn limits_and_a_start_are_listed() {
        let mut draft = discount(standard(Target::Subtotal, Value::Percent(bp(1250))));
        draft.limits = Limits {
            total: Some(1),
            once_per_customer: true,
        };
        draft.starts_at = Some(Timestamp::UNIX_EPOCH);
        draft.combines_with = ClassSet::ALL;
        assert_eq!(
            of(draft),
            "12.5% off the order · 1 use · once per customer · from 1 Jan 1970"
        );
    }

    #[test]
    fn buy_x_get_y_names_both_sides() {
        let draft = discount(Kind::BuyXGetY {
            buys: categories(&[1]),
            buy: Threshold::Quantity(1),
            gets: ItemSet {
                items: Items::Products(vec![prod(1)]),
                exclude_on_sale: false,
            },
            get_quantity: 1,
            value: Value::Percent(bp(10_000)),
            uses_per_order: Some(1),
        });
        assert_eq!(
            of(draft),
            "Buy 1 from Grow kits, get 1 Liquid Culture Syringe free (once per order)"
        );
    }

    #[test]
    fn buy_x_get_y_of_anything_counts_items() {
        let draft = discount(Kind::BuyXGetY {
            buys: ItemSet::all(),
            buy: Threshold::Quantity(2),
            gets: ItemSet::all(),
            get_quantity: 1,
            value: Value::Percent(bp(5000)),
            uses_per_order: Some(3),
        });
        assert_eq!(
            of(draft),
            "Buy 2 items, get 1 item at 50% off (up to 3 times per order)"
        );
    }

    #[test]
    fn buy_x_get_y_by_amount_says_spend() {
        let draft = discount(Kind::BuyXGetY {
            buys: categories(&[1]),
            buy: Threshold::Amount(PerMarket::of(mkt(1), eur(5000))),
            gets: ItemSet::all(),
            get_quantity: 1,
            value: Value::Percent(bp(10_000)),
            uses_per_order: None,
        });
        assert_eq!(of(draft), "Spend €50 on Grow kits, get 1 item free");
    }

    #[test]
    fn tiers_list_each_step_then_the_items() {
        let draft = discount(Kind::Tiered {
            items: categories(&[2]),
            measure: Measure::Quantity,
            tiers: vec![
                crate::Tier {
                    threshold: Threshold::Quantity(3),
                    value: Value::Percent(bp(1000)),
                },
                crate::Tier {
                    threshold: Threshold::Quantity(6),
                    value: Value::Percent(bp(1500)),
                },
            ],
        });
        assert_eq!(of(draft), "10% off 3+ items, 15% off 6+ items in Cultures");
    }

    #[test]
    fn items_on_sale_can_be_left_out() {
        let mut set = categories(&[1]);
        set.exclude_on_sale = true;
        assert_eq!(of(percent_off(set)), "10% off Grow kits not on sale");
    }

    #[test]
    fn staff_see_the_groups_named() {
        let group = CustomerGroupId::from_uuid(Uuid::from_u128(1));
        let mut draft = discount(standard(Target::Subtotal, Value::Percent(bp(1000))));
        draft.conditions = vec![Condition::CustomerGroups(vec![group])];
        let mut names = names();
        names.customer_groups.insert(group, "VIP".into());
        assert_eq!(
            summary(&built(draft), Some(mkt(1)), &names, Audience::Staff),
            "10% off the order for VIP"
        );
    }

    #[test]
    fn shoppers_never_see_which_groups() {
        let mut draft = discount(standard(Target::Subtotal, Value::Percent(bp(1000))));
        draft.conditions = vec![Condition::CustomerGroups(vec![CustomerGroupId::from_uuid(
            Uuid::from_u128(1),
        )])];
        assert_eq!(for_shoppers(draft), "10% off the order for members");
    }

    #[test]
    fn shoppers_never_see_which_customers() {
        let mut draft = discount(standard(Target::Subtotal, Value::Percent(bp(1000))));
        draft.conditions = vec![Condition::Customers(vec![CustomerId::from_uuid(
            Uuid::from_u128(1),
        )])];
        assert_eq!(
            for_shoppers(draft),
            "10% off the order for selected customers"
        );
    }

    #[test]
    fn a_shoppers_request_leaves_out_groups_and_customers() {
        let mut draft = percent_off(categories(&[1]));
        draft.conditions = vec![
            Condition::Customers(vec![CustomerId::from_uuid(Uuid::from_u128(1))]),
            Condition::CustomerGroups(vec![CustomerGroupId::from_uuid(Uuid::from_u128(1))]),
        ];
        let d = built(draft);
        let request = NameRequest::for_shoppers([&d]);
        assert_eq!(
            request,
            NameRequest {
                categories: vec![cat(1)],
                active_products_only: true,
                ..NameRequest::default()
            }
        );
    }

    #[test]
    fn a_request_lists_each_id_once() {
        let a = built(percent_off(categories(&[2, 1])));
        let b = built(percent_off(categories(&[1])));
        assert_eq!(
            NameRequest::for_staff([&a, &b]).categories,
            vec![cat(1), cat(2)]
        );
    }

    #[test]
    fn percentages_keep_only_the_digits_they_need() {
        assert_eq!(percent(bp(1000)), "10%");
        assert_eq!(percent(bp(1250)), "12.5%");
        assert_eq!(percent(bp(1234)), "12.34%");
        assert_eq!(percent(bp(5)), "0.05%");
    }

    #[test]
    fn money_uses_the_currencys_minor_units() {
        assert_eq!(money(eur(5)), "€0.05");
        assert_eq!(money(of_currency(1_234_567, "KWD")), "1,234.567 KWD");
        assert_eq!(money(of_currency(600, "JPY")), "600 JPY");
    }

    #[test]
    fn whole_amounts_drop_their_zeros() {
        assert_eq!(money(eur(5000)), "€50");
        assert_eq!(money(of_currency(600_000, "RSD")), "6,000 RSD");
    }

    #[test]
    fn thousands_are_grouped() {
        assert_eq!(money(eur(123_456_789)), "€1,234,567.89");
        assert_eq!(money(of_currency(100_000, "USD")), "$1,000");
    }

    #[test]
    fn dates_are_utc_days() {
        assert_eq!(
            date(Timestamp::from_unix_micros(951_782_400_000_000)),
            "29 Feb 2000"
        );
        assert_eq!(date(Timestamp::from_unix_micros(-1)), "31 Dec 1969");
    }
}
