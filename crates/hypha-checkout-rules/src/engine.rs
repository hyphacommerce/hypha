//! Evaluating rules against a cart: a pure function over plain values, so it runs on
//! every read of a cart and the admin's "Try an address" alike.

use std::collections::HashSet;

use hypha_core::{
    AddressField, AddressFlags, AddressParts, AddressVerdict, CategoryId, ChannelId, CountryCode,
    MarketId, Message, MessagePath, MessageSource, Money, ProductId, ShippingOptionId, Weight,
};

use crate::rule::{AddressFlag, CheckoutRule, RuleCondition, RuleCountries, RuleField, compact};

/// What verification found, as rules read it: what isn't known is `None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VerifiedFacts {
    pub verdict: Option<AddressVerdict>,
    pub po_box: Option<bool>,
    pub residential: Option<bool>,
    pub business: Option<bool>,
    pub vacant: Option<bool>,
}

/// A line as rules read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleLine {
    pub product: ProductId,
    /// Its product's categories with their ancestors; empty unless some rule names
    /// categories.
    pub categories: HashSet<CategoryId>,
    pub quantity: u32,
    /// One unit's weight when it's shipped; `None` when it isn't, or has no weight.
    pub shipping_weight: Option<Weight>,
}

/// Everything rules read about a cart, as plain values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleInput {
    /// The shipping address, normalised (its region as a code); `None` before there's one.
    pub address: Option<AddressParts>,
    /// What the address's text tells.
    pub flags: AddressFlags,
    /// What verification found; `None` when nothing was kept.
    pub verification: Option<VerifiedFacts>,
    pub email: Option<String>,
    /// The cart's country: the address's counts once there is one.
    pub country: CountryCode,
    pub market: Option<MarketId>,
    pub channel: Option<ChannelId>,
    /// The chosen option of each shipping group, in the cart's order.
    pub shipping: Vec<Option<ShippingOptionId>>,
    pub lines: Vec<RuleLine>,
    /// After discounts; `None` when it isn't known (a test without lines).
    pub subtotal: Option<Money>,
}

impl RuleInput {
    fn country(&self) -> CountryCode {
        self.address
            .as_ref()
            .and_then(|a| CountryCode::new(&a.country).ok())
            .unwrap_or(self.country)
    }

    fn value(&self, field: RuleField) -> Option<&str> {
        match field.address_field() {
            Some(f) => self.address.as_ref().and_then(|a| f.of(a)),
            None => self.email.as_deref().filter(|e| !e.is_empty()),
        }
    }

    fn flag(&self, flag: AddressFlag) -> bool {
        let verified = self.verification.unwrap_or_default();
        let yes = |v: Option<bool>| v == Some(true);
        match flag {
            AddressFlag::PoBox => self.flags.po_box || yes(verified.po_box),
            AddressFlag::Military => self.flags.military,
            AddressFlag::ParcelLocker => self.flags.parcel_locker,
            AddressFlag::NonLatin => self.flags.non_latin,
            AddressFlag::Residential => yes(verified.residential),
            AddressFlag::Business => yes(verified.business),
            AddressFlag::Vacant => yes(verified.vacant),
        }
    }

    /// Grams of what's shipped.
    fn weight(&self) -> u64 {
        self.lines
            .iter()
            .filter_map(|l| {
                l.shipping_weight
                    .map(|w| u64::from(w.grams()) * u64::from(l.quantity))
            })
            .sum()
    }

    /// The postal code as rules compare it, without a leading country prefix
    /// (`ES-28039` is `28039`).
    fn postal_code(&self) -> Option<String> {
        let code = compact(self.value(RuleField::PostalCode)?);
        let prefix = self.country().as_str().to_string();
        Some(match code.strip_prefix(&prefix) {
            Some(rest) if rest.starts_with(|c: char| c.is_ascii_digit()) => rest.to_string(),
            _ => code,
        })
    }

    fn line_matches(line: &RuleLine, products: &[ProductId], categories: &[CategoryId]) -> bool {
        products.contains(&line.product) || categories.iter().any(|c| line.categories.contains(c))
    }
}

/// The messages `rules` give `input`, in the rules' order: each enabled rule whose
/// `applies_to` matches and whose conditions all hold adds its message. A rule reading the
/// address waits for one.
pub fn evaluate(input: &RuleInput, rules: &[CheckoutRule]) -> Vec<Message> {
    rules
        .iter()
        .filter(|rule| rule.enabled())
        .filter(|rule| input.address.is_some() || !rule.reads_address())
        .filter(|rule| applies(input, rule))
        .filter(|rule| {
            rule.conditions()
                .iter()
                .enumerate()
                .all(|(i, c)| holds(input, rule, i, c))
        })
        .filter_map(|rule| {
            Message::new(
                rule.severity(),
                rule.code().clone(),
                rule.path().unwrap_or_else(|| default_path(input, rule)),
                &rule.outcome().message,
                MessageSource::Rule { rule: rule.id() },
            )
            // The text was checked when the rule was saved, so it always makes a message.
            .ok()
        })
        .collect()
}

/// Whether some enabled rule names categories: only then are the lines' categories read.
pub fn names_categories(rules: &[CheckoutRule]) -> bool {
    rules.iter().any(|r| r.enabled() && r.names_categories())
}

/// Whether `value` is listed; an empty list takes anything, a missing value nothing else.
fn listed<T: PartialEq>(list: &[T], value: Option<T>) -> bool {
    list.is_empty() || value.is_some_and(|v| list.contains(&v))
}

fn applies(input: &RuleInput, rule: &CheckoutRule) -> bool {
    let to = rule.applies_to();
    let country = input.country();
    let country_ok = match &to.countries {
        RuleCountries::All => true,
        RuleCountries::Include { codes } => codes.contains(&country),
        RuleCountries::Exclude { codes } => !codes.contains(&country),
    };
    country_ok
        && listed(&to.markets, input.market)
        && listed(&to.channels, input.channel)
        && (to.shipping_options.is_empty() || chosen_group(input, rule).is_some())
}

/// The first group whose chosen option is one the rule is for.
fn chosen_group(input: &RuleInput, rule: &CheckoutRule) -> Option<usize> {
    let options = &rule.applies_to().shipping_options;
    input
        .shipping
        .iter()
        .position(|o| o.is_some_and(|o| options.contains(&o)))
}

fn holds(input: &RuleInput, rule: &CheckoutRule, i: usize, condition: &RuleCondition) -> bool {
    let text = |field: RuleField| input.value(field).unwrap_or("");
    match condition {
        RuleCondition::FieldMissing { field } => input.value(*field).is_none(),
        RuleCondition::FieldPresent { field } => input.value(*field).is_some(),
        RuleCondition::FieldMatches { field, .. } => {
            rule.regex(i).is_some_and(|r| r.is_match(text(*field)))
        }
        RuleCondition::FieldNotMatches { field, .. } => {
            rule.regex(i).is_some_and(|r| !r.is_match(text(*field)))
        }
        RuleCondition::FieldContains { field, values } => contains(text(*field), values),
        RuleCondition::FieldNotContains { field, values } => !contains(text(*field), values),
        RuleCondition::FieldLongerThan { field, length } => {
            chars(text(*field)) > u64::from(*length)
        }
        RuleCondition::FieldShorterThan { field, length } => {
            chars(text(*field)) < u64::from(*length)
        }
        RuleCondition::PostalCodeIn {
            prefixes,
            ranges,
            codes,
        } => input
            .postal_code()
            .is_some_and(|code| postal_in(&code, prefixes, ranges, codes)),
        RuleCondition::PostalCodeNotIn {
            prefixes,
            ranges,
            codes,
        } => !input
            .postal_code()
            .is_some_and(|code| postal_in(&code, prefixes, ranges, codes)),
        RuleCondition::RegionIn { regions } => region_in(input, regions),
        RuleCondition::RegionNotIn { regions } => !region_in(input, regions),
        RuleCondition::AddressIs { flag } => input.flag(*flag),
        RuleCondition::AddressIsNot { flag } => !input.flag(*flag),
        RuleCondition::VerificationIs { verdicts } => input
            .verification
            .and_then(|v| v.verdict)
            .is_some_and(|v| verdicts.contains(&v)),
        RuleCondition::CartContains {
            products,
            categories,
        } => input
            .lines
            .iter()
            .any(|l| RuleInput::line_matches(l, products, categories)),
        RuleCondition::CartNotContains {
            products,
            categories,
        } => !input
            .lines
            .iter()
            .any(|l| RuleInput::line_matches(l, products, categories)),
        RuleCondition::SubtotalAtLeast { amounts } => {
            subtotal_against(input, amounts, |subtotal, amount| subtotal >= amount)
        }
        RuleCondition::SubtotalBelow { amounts } => {
            subtotal_against(input, amounts, |subtotal, amount| subtotal < amount)
        }
        RuleCondition::WeightAtLeast { grams } => input.weight() >= *grams,
        RuleCondition::WeightBelow { grams } => input.weight() < *grams,
    }
}

fn chars(text: &str) -> u64 {
    u64::try_from(text.chars().count()).unwrap_or(u64::MAX)
}

fn contains(text: &str, values: &[String]) -> bool {
    let text = text.to_lowercase();
    values.iter().any(|v| text.contains(&v.to_lowercase()))
}

fn postal_in(
    code: &str,
    prefixes: &[String],
    ranges: &[crate::rule::PostalRange],
    codes: &[String],
) -> bool {
    let len = code.chars().count();
    prefixes.iter().any(|p| code.starts_with(&compact(p)))
        || codes.iter().any(|c| code == compact(c))
        || ranges.iter().any(|r| {
            let (from, to) = (compact(&r.from), compact(&r.to));
            // Text of one length compares as numbers do when it's all digits.
            from.chars().count() == len
                && to.chars().count() == len
                && from.as_str() <= code
                && code <= to.as_str()
        })
}

fn region_in(input: &RuleInput, regions: &[String]) -> bool {
    input.value(RuleField::Province).is_some_and(|region| {
        regions
            .iter()
            .any(|r| r.eq_ignore_ascii_case(region.trim()))
    })
}

fn subtotal_against(
    input: &RuleInput,
    amounts: &[Money],
    compare: impl Fn(i64, i64) -> bool,
) -> bool {
    let Some(subtotal) = input.subtotal else {
        return false;
    };
    amounts
        .iter()
        .find(|a| a.currency() == subtotal.currency())
        .is_some_and(|a| compare(subtotal.amount(), a.amount()))
}

/// Where a rule's message points when the merchant didn't say: the group of the option
/// it's for, else what its first condition reads.
fn default_path(input: &RuleInput, rule: &CheckoutRule) -> MessagePath {
    if let Some(group) = chosen_group(input, rule).and_then(|g| u32::try_from(g).ok()) {
        return MessagePath::ShippingGroup(group);
    }
    let Some(first) = rule.conditions().first() else {
        return MessagePath::Cart;
    };
    let field = |f: AddressField| MessagePath::ShippingAddressField(f);
    match first {
        RuleCondition::FieldMissing { field: f }
        | RuleCondition::FieldPresent { field: f }
        | RuleCondition::FieldMatches { field: f, .. }
        | RuleCondition::FieldNotMatches { field: f, .. }
        | RuleCondition::FieldContains { field: f, .. }
        | RuleCondition::FieldNotContains { field: f, .. }
        | RuleCondition::FieldLongerThan { field: f, .. }
        | RuleCondition::FieldShorterThan { field: f, .. } => f.path(),
        RuleCondition::PostalCodeIn { .. } | RuleCondition::PostalCodeNotIn { .. } => {
            field(AddressField::PostalCode)
        }
        RuleCondition::RegionIn { .. } | RuleCondition::RegionNotIn { .. } => {
            field(AddressField::Province)
        }
        RuleCondition::AddressIs { .. } | RuleCondition::AddressIsNot { .. } => {
            field(AddressField::Line1)
        }
        RuleCondition::VerificationIs { .. } => MessagePath::ShippingAddress,
        RuleCondition::CartContains {
            products,
            categories,
        } => input
            .lines
            .iter()
            .position(|l| RuleInput::line_matches(l, products, categories))
            .and_then(|i| u32::try_from(i).ok())
            .map_or(MessagePath::Cart, MessagePath::Line),
        RuleCondition::CartNotContains { .. }
        | RuleCondition::SubtotalAtLeast { .. }
        | RuleCondition::SubtotalBelow { .. }
        | RuleCondition::WeightAtLeast { .. }
        | RuleCondition::WeightBelow { .. } => MessagePath::Cart,
    }
}

#[cfg(test)]
mod tests;
