//! A checkout rule: where it applies, the conditions that must all hold, and what the
//! shopper is told when they do. Rules are typed data, not an expression language, so a
//! merchant builds one from a form and an agent from a schema.

use std::collections::{BTreeSet, HashSet};
use std::hash::Hash;
use std::sync::OnceLock;

use hypha_core::{
    AddressField, AddressVerdict, CategoryId, ChannelId, CheckoutRuleId, CountryCode, Currency,
    MarketId, MessageCode, MessagePath, Money, PendingEvent, ProductId, Recorded, Severity,
    ShippingOptionId, Stamped, Stamps, Subject, Version, Versioned, required_text,
};
use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};

use crate::{
    error::{CheckoutRuleError, Problem},
    events::{CHECKOUT_RULE, CheckoutRuleCreated, CheckoutRuleDeleted, CheckoutRuleUpdated},
};

/// The most rules a store has: every read of a cart evaluates them all.
pub const MAX_RULES: usize = 100;
pub const MAX_CONDITIONS: usize = 10;
pub const MAX_MESSAGE_CHARS: usize = 300;
pub const MAX_PATTERN_CHARS: usize = 300;
/// Strings a `field_contains` looks for.
pub const MAX_VALUES: usize = 50;
/// Prefixes, ranges and codes of a postal code condition together.
pub const MAX_POSTAL_ENTRIES: usize = 1000;
/// Countries, regions, markets, channels, options, products and categories a list names.
pub const MAX_LIST: usize = 250;
/// The longest a field length condition counts to: an address field is at most this long.
pub const MAX_FIELD_LENGTH: u32 = 255;
/// Amounts, one per currency, a subtotal condition lists.
pub const MAX_AMOUNTS: usize = 50;
const MAX_POSTAL_CODE_CHARS: usize = 20;
/// What a compiled pattern may take: plenty for an address pattern, and a bound on what a
/// pathological one costs to compile.
const REGEX_SIZE_LIMIT: usize = 256 * 1024;

/// A field a condition reads: one of the shipping address's, or the cart's email.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum RuleField {
    FirstName,
    LastName,
    Company,
    Line1,
    Line2,
    City,
    Province,
    PostalCode,
    Country,
    Phone,
    Email,
}

impl RuleField {
    /// The address field it is; `None` for the email.
    pub fn address_field(self) -> Option<AddressField> {
        Some(match self {
            RuleField::FirstName => AddressField::FirstName,
            RuleField::LastName => AddressField::LastName,
            RuleField::Company => AddressField::Company,
            RuleField::Line1 => AddressField::Line1,
            RuleField::Line2 => AddressField::Line2,
            RuleField::City => AddressField::City,
            RuleField::Province => AddressField::Province,
            RuleField::PostalCode => AddressField::PostalCode,
            RuleField::Country => AddressField::Country,
            RuleField::Phone => AddressField::Phone,
            RuleField::Email => return None,
        })
    }

    /// Where a message about the field points.
    pub fn path(self) -> MessagePath {
        match self.address_field() {
            Some(field) => MessagePath::ShippingAddressField(field),
            None => MessagePath::Email,
        }
    }
}

/// Something an address is, as its text tells (`po_box`, `military`, `parcel_locker`,
/// `non_latin`) or as verification found (`residential`, `business`, `vacant`; `po_box`
/// from either). What isn't known counts as not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum AddressFlag {
    PoBox,
    Military,
    ParcelLocker,
    NonLatin,
    Residential,
    Business,
    Vacant,
}

/// Which shipping countries a rule applies to.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum RuleCountries {
    #[default]
    All,
    /// Only these.
    Include { codes: Vec<CountryCode> },
    /// Every country but these.
    Exclude { codes: Vec<CountryCode> },
}

/// Where a rule applies; an empty list doesn't narrow it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RuleAppliesTo {
    /// The shipping address's country, or the cart's before it has one.
    #[serde(default)]
    pub countries: RuleCountries,
    #[serde(default)]
    pub markets: Vec<MarketId>,
    #[serde(default)]
    pub channels: Vec<ChannelId>,
    /// Only while one of these is the chosen shipping option.
    #[serde(default)]
    pub shipping_options: Vec<ShippingOptionId>,
}

/// Postal codes from `from` to `to`, both included, compared as text of the same length
/// (so numerically when both are digits); spaces and hyphens are ignored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PostalRange {
    pub from: String,
    pub to: String,
}

/// One thing that must hold for a rule's message. Text comparisons ignore case unless a
/// pattern says otherwise; an empty field reads as empty text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RuleCondition {
    /// The field is empty.
    FieldMissing { field: RuleField },
    /// The field isn't empty.
    FieldPresent { field: RuleField },
    /// The pattern (the `regex` crate's syntax) matches somewhere in the field.
    FieldMatches {
        field: RuleField,
        pattern: String,
        #[serde(default)]
        case_insensitive: bool,
    },
    /// The pattern matches nowhere in the field.
    FieldNotMatches {
        field: RuleField,
        pattern: String,
        #[serde(default)]
        case_insensitive: bool,
    },
    /// Any of the values is in the field, ignoring case.
    FieldContains {
        field: RuleField,
        values: Vec<String>,
    },
    /// None of the values is in the field, ignoring case.
    FieldNotContains {
        field: RuleField,
        values: Vec<String>,
    },
    /// The field has more than `length` characters.
    FieldLongerThan { field: RuleField, length: u32 },
    /// The field has fewer than `length` characters.
    FieldShorterThan { field: RuleField, length: u32 },
    /// The postal code starts with one of the prefixes, is in one of the ranges, or is one
    /// of the codes; spaces, hyphens and case are ignored.
    PostalCodeIn {
        #[serde(default)]
        prefixes: Vec<String>,
        #[serde(default)]
        ranges: Vec<PostalRange>,
        #[serde(default)]
        codes: Vec<String>,
    },
    /// The postal code is none of them (or there's none).
    PostalCodeNotIn {
        #[serde(default)]
        prefixes: Vec<String>,
        #[serde(default)]
        ranges: Vec<PostalRange>,
        #[serde(default)]
        codes: Vec<String>,
    },
    /// The region's code is one of them, ignoring case.
    RegionIn { regions: Vec<String> },
    /// The region's code is none of them (or there's none).
    RegionNotIn { regions: Vec<String> },
    /// The address is that.
    AddressIs { flag: AddressFlag },
    /// The address isn't that, or it isn't known.
    AddressIsNot { flag: AddressFlag },
    /// Verification's verdict is one of them; none kept doesn't hold.
    VerificationIs { verdicts: Vec<AddressVerdict> },
    /// A line is one of the products, or in one of the categories (or below one).
    CartContains {
        #[serde(default)]
        products: Vec<ProductId>,
        #[serde(default)]
        categories: Vec<CategoryId>,
    },
    /// No line is any of the products, or in any of the categories.
    CartNotContains {
        #[serde(default)]
        products: Vec<ProductId>,
        #[serde(default)]
        categories: Vec<CategoryId>,
    },
    /// The subtotal (after discounts) is at least the amount in its currency; a currency
    /// not listed doesn't hold.
    SubtotalAtLeast { amounts: Vec<Money> },
    /// The subtotal is below the amount in its currency; a currency not listed doesn't
    /// hold.
    SubtotalBelow { amounts: Vec<Money> },
    /// What's shipped weighs at least this; lines without a weight count nothing.
    WeightAtLeast { grams: u64 },
    /// What's shipped weighs less than this.
    WeightBelow { grams: u64 },
}

impl RuleCondition {
    /// Whether it reads the shipping address: a rule with one waits for an address.
    pub fn reads_address(&self) -> bool {
        match self {
            RuleCondition::FieldMissing { field }
            | RuleCondition::FieldPresent { field }
            | RuleCondition::FieldMatches { field, .. }
            | RuleCondition::FieldNotMatches { field, .. }
            | RuleCondition::FieldContains { field, .. }
            | RuleCondition::FieldNotContains { field, .. }
            | RuleCondition::FieldLongerThan { field, .. }
            | RuleCondition::FieldShorterThan { field, .. } => field.address_field().is_some(),
            RuleCondition::PostalCodeIn { .. }
            | RuleCondition::PostalCodeNotIn { .. }
            | RuleCondition::RegionIn { .. }
            | RuleCondition::RegionNotIn { .. }
            | RuleCondition::AddressIs { .. }
            | RuleCondition::AddressIsNot { .. }
            | RuleCondition::VerificationIs { .. } => true,
            RuleCondition::CartContains { .. }
            | RuleCondition::CartNotContains { .. }
            | RuleCondition::SubtotalAtLeast { .. }
            | RuleCondition::SubtotalBelow { .. }
            | RuleCondition::WeightAtLeast { .. }
            | RuleCondition::WeightBelow { .. } => false,
        }
    }

    /// The categories it names, which the cart's lines are then read in.
    pub fn categories(&self) -> &[CategoryId] {
        match self {
            RuleCondition::CartContains { categories, .. }
            | RuleCondition::CartNotContains { categories, .. } => categories,
            _ => &[],
        }
    }

    /// Its pattern and case, for the conditions that have one.
    fn pattern(&self) -> Option<(&str, bool)> {
        match self {
            RuleCondition::FieldMatches {
                pattern,
                case_insensitive,
                ..
            }
            | RuleCondition::FieldNotMatches {
                pattern,
                case_insensitive,
                ..
            } => Some((pattern, *case_insensitive)),
            _ => None,
        }
    }
}

/// What the shopper is told when a rule's conditions hold.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RuleOutcome {
    /// `error` blocks checkout; `warning` never does.
    pub severity: Severity,
    /// Plain text for the shopper, 1 to 300 characters.
    pub message: String,
    /// Where the message points (a cart message path, not a line's). `null`: the chosen
    /// shipping option's group when the rule is for options, else the field the first
    /// condition reads (the first street line for what an address is, the whole address
    /// for its verification, the first matching line for what the cart contains), else the
    /// whole cart.
    #[serde(default)]
    pub path: Option<String>,
    /// The message's code for clients: snake_case; `null` is `rule`.
    #[serde(default)]
    pub code: Option<String>,
}

/// A rule as written: what creating or replacing one takes, and what storage keeps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleDraft {
    pub name: String,
    pub enabled: bool,
    pub applies_to: RuleAppliesTo,
    pub conditions: Vec<RuleCondition>,
    pub outcome: RuleOutcome,
}

/// A rule's compiled pattern per condition, compiled when the rule is saved and on first
/// use after it's loaded: compiling every pattern of every rule on each read of a cart
/// would cost more than evaluating them.
#[derive(Debug, Clone, Default)]
struct Patterns(Vec<OnceLock<Option<Regex>>>);

impl PartialEq for Patterns {
    /// A cache of what the conditions say: equal rules compile alike.
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for Patterns {}

fn compile(pattern: &str, case_insensitive: bool) -> Result<Regex, regex::Error> {
    RegexBuilder::new(pattern)
        .case_insensitive(case_insensitive)
        .size_limit(REGEX_SIZE_LIMIT)
        .build()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Checked {
    draft: RuleDraft,
    path: Option<MessagePath>,
    code: MessageCode,
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckoutRule {
    id: CheckoutRuleId,
    version: Version,
    stamps: Option<Stamps>,
    position: u32,
    rule: Checked,
    patterns: Patterns,
    changes: Changes,
}

impl Recorded for CheckoutRule {
    fn take_events(&mut self) -> Vec<PendingEvent> {
        let changes = std::mem::take(&mut self.changes);
        let subject = Subject::new(CHECKOUT_RULE, self.id.as_uuid());
        let checkout_rule = self.id;
        if changes.deleted {
            return vec![PendingEvent::new(
                subject,
                &CheckoutRuleDeleted { checkout_rule },
            )];
        }
        if changes.created {
            return vec![PendingEvent::new(
                subject,
                &CheckoutRuleCreated { checkout_rule },
            )];
        }
        if changes.fields.is_empty() {
            return Vec::new();
        }
        let event = CheckoutRuleUpdated {
            checkout_rule,
            fields: changes.fields.into_iter().collect(),
        };
        vec![PendingEvent::new(subject, &event)]
    }
}

impl Versioned for CheckoutRule {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

impl Stamped for CheckoutRule {
    fn stamps(&self) -> Option<Stamps> {
        self.stamps
    }

    fn set_stamps(&mut self, stamps: Stamps) {
        self.stamps = Some(stamps);
    }
}

impl CheckoutRule {
    /// The code a rule's message has when the merchant gives none.
    pub const DEFAULT_CODE: &str = "rule";

    /// A new rule at `position` (its place in the list, from 0), every rule of the draft
    /// checked at once.
    pub fn new(
        id: CheckoutRuleId,
        position: u32,
        draft: RuleDraft,
    ) -> Result<Self, CheckoutRuleError> {
        let (rule, patterns) = check(draft, true)?;
        Ok(Self {
            id,
            version: Version::NEW,
            stamps: None,
            position,
            rule,
            patterns,
            changes: Changes {
                created: true,
                ..Changes::default()
            },
        })
    }

    /// Replaces what the rule says, every rule of the draft checked at once.
    pub fn update(&mut self, draft: RuleDraft) -> Result<(), CheckoutRuleError> {
        let (new, patterns) = check(draft, true)?;
        let old = &self.rule.draft;
        let changed = [
            ("name", new.draft.name != old.name),
            ("enabled", new.draft.enabled != old.enabled),
            ("applies_to", new.draft.applies_to != old.applies_to),
            ("conditions", new.draft.conditions != old.conditions),
            ("outcome", new.draft.outcome != old.outcome),
        ];
        self.changes.fields.extend(
            changed
                .into_iter()
                .filter(|(_, changed)| *changed)
                .map(|(field, _)| field),
        );
        self.rule = new;
        self.patterns = patterns;
        Ok(())
    }

    /// Moves it to `position` in the list.
    pub fn set_position(&mut self, position: u32) {
        if position != self.position {
            self.position = position;
            self.changes.fields.insert("position");
        }
    }

    /// Whether it has changes not yet written.
    pub fn has_changes(&self) -> bool {
        self.changes.created || self.changes.deleted || !self.changes.fields.is_empty()
    }

    /// Records `checkout_rule.deleted`, for the repo's delete.
    pub fn mark_deleted(&mut self) {
        self.changes.deleted = true;
    }

    pub fn id(&self) -> CheckoutRuleId {
        self.id
    }

    pub fn position(&self) -> u32 {
        self.position
    }

    pub fn name(&self) -> &str {
        &self.rule.draft.name
    }

    pub fn enabled(&self) -> bool {
        self.rule.draft.enabled
    }

    pub fn applies_to(&self) -> &RuleAppliesTo {
        &self.rule.draft.applies_to
    }

    pub fn conditions(&self) -> &[RuleCondition] {
        &self.rule.draft.conditions
    }

    pub fn outcome(&self) -> &RuleOutcome {
        &self.rule.draft.outcome
    }

    pub fn severity(&self) -> Severity {
        self.rule.draft.outcome.severity
    }

    /// The path the merchant chose; `None`: where `RuleOutcome::path` says.
    pub fn path(&self) -> Option<MessagePath> {
        self.rule.path
    }

    /// The message's code: the merchant's, or `rule`.
    pub fn code(&self) -> &MessageCode {
        &self.rule.code
    }

    /// The rule as written, for storage and for replacing parts of it.
    pub fn draft(&self) -> &RuleDraft {
        &self.rule.draft
    }

    /// Whether some condition reads the shipping address.
    pub fn reads_address(&self) -> bool {
        self.conditions().iter().any(RuleCondition::reads_address)
    }

    /// Whether some condition names categories.
    pub fn names_categories(&self) -> bool {
        self.conditions().iter().any(|c| !c.categories().is_empty())
    }

    /// Condition `i`'s compiled pattern; `None` for one without a pattern, or (after
    /// loading) one this build can no longer compile, which then never holds.
    pub(crate) fn regex(&self, i: usize) -> Option<&Regex> {
        let (pattern, case_insensitive) = self.conditions().get(i)?.pattern()?;
        self.patterns
            .0
            .get(i)?
            .get_or_init(|| compile(pattern, case_insensitive).ok())
            .as_ref()
    }

    pub fn snapshot(&self) -> CheckoutRuleSnapshot {
        CheckoutRuleSnapshot {
            id: self.id,
            version: self.version,
            stamps: self.stamps,
            position: self.position,
            draft: self.rule.draft.clone(),
        }
    }

    /// A stored rule, its rules checked again but its patterns compiled on first use.
    pub fn restore(s: CheckoutRuleSnapshot) -> Result<Self, CheckoutRuleError> {
        let (rule, patterns) = check(s.draft, false)?;
        Ok(Self {
            id: s.id,
            version: s.version,
            stamps: s.stamps,
            position: s.position,
            rule,
            patterns,
            changes: Changes::default(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckoutRuleSnapshot {
    pub id: CheckoutRuleId,
    pub version: Version,
    pub stamps: Option<Stamps>,
    pub position: u32,
    pub draft: RuleDraft,
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
}

/// A list of at most `max`, each once; `required` refuses an empty one.
fn check_list<T: Eq + Hash>(items: &[T], path: &str, max: usize, required: bool, p: &mut Problems) {
    if required && items.is_empty() {
        p.add(path, "empty", "list at least one");
    } else if items.len() > max {
        p.add(path, "too_many", format!("at most {max}"));
    } else {
        let mut seen = HashSet::with_capacity(items.len());
        if let Some(i) = items.iter().position(|item| !seen.insert(item)) {
            p.add(format!("{path}[{i}]"), "duplicate", "listed twice");
        }
    }
}

/// Text entries: trimmed, each 1 to `max_chars` characters, upper-cased when `upper`.
fn check_texts(texts: &mut [String], path: &str, max_chars: usize, upper: bool, p: &mut Problems) {
    for (i, text) in texts.iter_mut().enumerate() {
        let trimmed = text.trim();
        *text = if upper {
            trimmed.to_uppercase()
        } else {
            trimmed.to_string()
        };
        if text.is_empty() {
            p.add(format!("{path}[{i}]"), "empty", "write something");
        } else if text.chars().count() > max_chars {
            p.add(
                format!("{path}[{i}]"),
                "too_long",
                format!("at most {max_chars} characters"),
            );
        }
    }
}

fn check_postal(
    prefixes: &mut [String],
    ranges: &mut [PostalRange],
    codes: &mut [String],
    path: &str,
    p: &mut Problems,
) {
    let count = prefixes.len() + ranges.len() + codes.len();
    if count == 0 {
        p.add(
            path,
            "empty",
            "list at least one prefix, range or postal code",
        );
    } else if count > MAX_POSTAL_ENTRIES {
        p.add(
            path,
            "too_many",
            format!("at most {MAX_POSTAL_ENTRIES} prefixes, ranges and codes together"),
        );
    }
    let max = MAX_POSTAL_CODE_CHARS;
    check_texts(prefixes, &format!("{path}.prefixes"), max, true, p);
    check_texts(codes, &format!("{path}.codes"), max, true, p);
    for (i, range) in ranges.iter_mut().enumerate() {
        let at = format!("{path}.ranges[{i}]");
        let mut ends = [
            std::mem::take(&mut range.from),
            std::mem::take(&mut range.to),
        ];
        check_texts(&mut ends, &at, max, true, p);
        let [from, to] = ends;
        let (a, b) = (compact(&from), compact(&to));
        if !a.is_empty() && !b.is_empty() {
            if a.chars().count() != b.chars().count() {
                p.add(
                    &at,
                    "lengths_differ",
                    "a range's ends have the same number of characters",
                );
            } else if a > b {
                p.add(&at, "not_ascending", "a range starts at or before its end");
            }
        }
        range.from = from;
        range.to = to;
    }
}

/// A postal code as compared: upper-cased, without spaces or hyphens.
pub(crate) fn compact(code: &str) -> String {
    code.chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .flat_map(char::to_uppercase)
        .collect()
}

fn check_amounts(amounts: &[Money], path: &str, p: &mut Problems) {
    if amounts.is_empty() {
        p.add(path, "empty", "give an amount for at least one currency");
    } else if amounts.len() > MAX_AMOUNTS {
        p.add(path, "too_many", format!("at most {MAX_AMOUNTS}"));
    }
    let mut seen: HashSet<Currency> = HashSet::new();
    for (i, amount) in amounts.iter().enumerate() {
        if amount.amount() < 0 {
            p.add(format!("{path}[{i}]"), "too_small", "at least zero");
        }
        if !seen.insert(amount.currency()) {
            p.add(
                format!("{path}[{i}]"),
                "duplicate",
                format!("{} is listed twice", amount.currency()),
            );
        }
    }
}

fn check_condition(
    condition: &mut RuleCondition,
    at: &str,
    compile_patterns: bool,
    p: &mut Problems,
) -> Option<Regex> {
    match condition {
        RuleCondition::FieldMissing { .. } | RuleCondition::FieldPresent { .. } => {}
        RuleCondition::FieldMatches {
            pattern,
            case_insensitive,
            ..
        }
        | RuleCondition::FieldNotMatches {
            pattern,
            case_insensitive,
            ..
        } => {
            let path = format!("{at}.pattern");
            if pattern.is_empty() {
                p.add(path, "empty", "write a pattern");
            } else if pattern.chars().count() > MAX_PATTERN_CHARS {
                p.add(
                    path,
                    "too_long",
                    format!("at most {MAX_PATTERN_CHARS} characters"),
                );
            } else if compile_patterns {
                match compile(pattern, *case_insensitive) {
                    Ok(regex) => return Some(regex),
                    Err(e) => p.add(path, "invalid_pattern", first_line(&e.to_string())),
                }
            }
        }
        RuleCondition::FieldContains { values, .. }
        | RuleCondition::FieldNotContains { values, .. } => {
            let path = format!("{at}.values");
            check_texts(values, &path, hypha_core::MAX_TEXT_LEN, false, p);
            let lowered: Vec<String> = values.iter().map(|v| v.to_lowercase()).collect();
            check_list(&lowered, &path, MAX_VALUES, true, p);
        }
        RuleCondition::FieldLongerThan { length, .. }
        | RuleCondition::FieldShorterThan { length, .. } => {
            if *length > MAX_FIELD_LENGTH {
                p.add(
                    format!("{at}.length"),
                    "out_of_range",
                    format!("at most {MAX_FIELD_LENGTH}"),
                );
            }
        }
        RuleCondition::PostalCodeIn {
            prefixes,
            ranges,
            codes,
        }
        | RuleCondition::PostalCodeNotIn {
            prefixes,
            ranges,
            codes,
        } => check_postal(prefixes, ranges, codes, at, p),
        RuleCondition::RegionIn { regions } | RuleCondition::RegionNotIn { regions } => {
            let path = format!("{at}.regions");
            check_texts(regions, &path, hypha_core::MAX_TEXT_LEN, true, p);
            check_list(regions, &path, MAX_LIST, true, p);
        }
        RuleCondition::AddressIs { .. } | RuleCondition::AddressIsNot { .. } => {}
        RuleCondition::VerificationIs { verdicts } => {
            check_list(verdicts, &format!("{at}.verdicts"), MAX_LIST, true, p);
        }
        RuleCondition::CartContains {
            products,
            categories,
        }
        | RuleCondition::CartNotContains {
            products,
            categories,
        } => {
            if products.is_empty() && categories.is_empty() {
                p.add(at, "empty", "list at least one product or category");
            } else if products.len() + categories.len() > MAX_LIST {
                p.add(
                    at,
                    "too_many",
                    format!("at most {MAX_LIST} products and categories together"),
                );
            }
            check_list(products, &format!("{at}.products"), MAX_LIST, false, p);
            check_list(categories, &format!("{at}.categories"), MAX_LIST, false, p);
        }
        RuleCondition::SubtotalAtLeast { amounts } | RuleCondition::SubtotalBelow { amounts } => {
            check_amounts(amounts, &format!("{at}.amounts"), p);
        }
        RuleCondition::WeightAtLeast { .. } | RuleCondition::WeightBelow { .. } => {}
    }
    None
}

/// The first line of a pattern's error: the `regex` crate draws the pattern above it.
fn first_line(error: &str) -> String {
    let line = error
        .lines()
        .rev()
        .find(|l| l.starts_with("error:"))
        .or_else(|| error.lines().next())
        .unwrap_or(error);
    line.trim_start_matches("error:").trim().to_string()
}

/// Where a rule's message may point: anywhere in the cart but a line, whose index
/// changes as the shopper edits the cart.
fn check_path(path: &str, p: &mut Problems) -> Option<MessagePath> {
    match path.trim().parse::<MessagePath>() {
        Ok(MessagePath::Line(_)) => {
            p.add(
                "outcome.path",
                "path_not_allowed",
                "a rule's message can't point at a line by its place",
            );
            None
        }
        Ok(path) => Some(path),
        Err(e) => {
            p.add("outcome.path", "invalid_path", e.to_string());
            None
        }
    }
}

fn check(
    mut draft: RuleDraft,
    compile_patterns: bool,
) -> Result<(Checked, Patterns), CheckoutRuleError> {
    let mut p = Problems::default();
    match required_text("name", &draft.name) {
        Ok(name) => draft.name = name,
        Err(e) => p.add("name", "invalid_text", e.to_string()),
    }

    let applies = &draft.applies_to;
    if let RuleCountries::Include { codes } | RuleCountries::Exclude { codes } = &applies.countries
    {
        check_list(codes, "applies_to.countries.codes", MAX_LIST, true, &mut p);
    }
    check_list(
        &applies.markets,
        "applies_to.markets",
        MAX_LIST,
        false,
        &mut p,
    );
    check_list(
        &applies.channels,
        "applies_to.channels",
        MAX_LIST,
        false,
        &mut p,
    );
    check_list(
        &applies.shipping_options,
        "applies_to.shipping_options",
        MAX_LIST,
        false,
        &mut p,
    );

    if draft.conditions.is_empty() {
        p.add("conditions", "empty", "add at least one condition");
    } else if draft.conditions.len() > MAX_CONDITIONS {
        p.add(
            "conditions",
            "too_many",
            format!("at most {MAX_CONDITIONS} conditions"),
        );
    }
    let mut patterns = Vec::with_capacity(draft.conditions.len());
    for (i, condition) in draft.conditions.iter_mut().enumerate() {
        let compiled = check_condition(
            condition,
            &format!("conditions[{i}]"),
            compile_patterns,
            &mut p,
        );
        let cell = OnceLock::new();
        if let Some(regex) = compiled {
            // Just made, so the cell is empty.
            let _ = cell.set(Some(regex));
        }
        patterns.push(cell);
    }

    let outcome = &mut draft.outcome;
    outcome.message = outcome.message.trim().to_string();
    let chars = outcome.message.chars().count();
    if chars == 0 {
        p.add(
            "outcome.message",
            "empty",
            "write the message the shopper sees",
        );
    } else if chars > MAX_MESSAGE_CHARS {
        p.add(
            "outcome.message",
            "too_long",
            format!("at most {MAX_MESSAGE_CHARS} characters"),
        );
    }
    outcome.path = outcome
        .path
        .take()
        .map(|path| path.trim().to_string())
        .filter(|path| !path.is_empty());
    let path = outcome
        .path
        .as_deref()
        .and_then(|path| check_path(path, &mut p));
    outcome.code = outcome
        .code
        .take()
        .map(|code| code.trim().to_string())
        .filter(|code| !code.is_empty());
    let code = match outcome.code.as_deref() {
        None => MessageCode::new(CheckoutRule::DEFAULT_CODE).ok(),
        Some(code) => match MessageCode::new(code) {
            Ok(code) => Some(code),
            Err(e) => {
                p.add("outcome.code", "invalid_code", e.to_string());
                None
            }
        },
    };

    match code {
        Some(code) if p.0.is_empty() => Ok((Checked { draft, path, code }, Patterns(patterns))),
        _ => Err(CheckoutRuleError::Invalid(p.0)),
    }
}

#[cfg(test)]
mod tests;
