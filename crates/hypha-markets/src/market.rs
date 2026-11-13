use std::collections::{BTreeSet, HashSet};

use hypha_core::{
    CountryCode, Currency, Language, MarketId, PendingEvent, Recorded, Stamped, Stamps, Subject,
    Version, Versioned, required_text,
};

use crate::{
    error::MarketError,
    events::{MARKET, MarketChanged},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Market {
    id: MarketId,
    version: Version,
    /// When it was stored and last changed, set by the adapter.
    stamps: Option<Stamps>,
    name: String,
    countries: Vec<CountryCode>,
    currency: Currency,
    prices_include_tax: bool,
    /// The languages shoppers are served here, the first being the market's default;
    /// empty for the store's default language alone.
    languages: Vec<Language>,
    changes: Changes,
}

/// The fields changed since the market was loaded (`created` for a new one), coalesced
/// into one `market.changed` per write: search rebuilds only for the fields documents
/// depend on. Compares as always equal, so it never makes two markets unequal.
#[derive(Debug, Clone, Default)]
struct Changes(BTreeSet<&'static str>);

impl PartialEq for Changes {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for Changes {}

impl Recorded for Market {
    fn take_events(&mut self) -> Vec<PendingEvent> {
        let fields = std::mem::take(&mut self.changes.0);
        if fields.is_empty() {
            return Vec::new();
        }
        let event = MarketChanged {
            market: self.id,
            fields: fields.into_iter().collect(),
        };
        vec![PendingEvent::new(
            Subject::new(MARKET, self.id.as_uuid()),
            &event,
        )]
    }
}

impl Versioned for Market {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

impl Stamped for Market {
    fn stamps(&self) -> Option<Stamps> {
        self.stamps
    }

    fn set_stamps(&mut self, stamps: Stamps) {
        self.stamps = Some(stamps);
    }
}

impl Market {
    pub fn new(
        id: MarketId,
        name: String,
        countries: Vec<CountryCode>,
        currency: Currency,
    ) -> Result<Self, MarketError> {
        if countries.is_empty() {
            return Err(MarketError::NoCountries);
        }

        let mut seen = HashSet::new();
        for country in &countries {
            if !seen.insert(*country) {
                return Err(MarketError::DuplicateCountry(*country));
            }
        }

        let mut market = Self {
            id,
            version: Version::NEW,
            stamps: None,
            name: required_text("name", &name)?,
            countries,
            currency,
            prices_include_tax: false,
            languages: Vec::new(),
            changes: Changes::default(),
        };
        market.changed("created");
        Ok(market)
    }

    fn changed(&mut self, field: &'static str) {
        self.changes.0.insert(field);
    }

    /// The shared text rule: trimmed, 1 to 255 characters.
    pub fn rename(&mut self, name: &str) -> Result<(), MarketError> {
        let name = required_text("name", name)?;
        if self.name != name {
            self.name = name;
            self.changed("name");
        }
        Ok(())
    }

    pub fn set_prices_include_tax(&mut self, include: bool) {
        if self.prices_include_tax != include {
            self.prices_include_tax = include;
            self.changed("prices_include_tax");
        }
    }

    /// The languages the market offers, in order (the first is its default); none means
    /// the store's default language alone. Each may be listed once. Whether the store
    /// offers them is the command's to check: a market can't see the store.
    pub fn set_languages(&mut self, languages: Vec<Language>) -> Result<(), MarketError> {
        for (i, language) in languages.iter().enumerate() {
            if languages[..i].contains(language) {
                return Err(MarketError::DuplicateLanguage(language.clone()));
            }
        }
        if self.languages != languages {
            self.languages = languages;
            self.changed("languages");
        }
        Ok(())
    }

    pub fn covers(&self, country: CountryCode) -> bool {
        self.countries.contains(&country)
    }

    pub fn add_country(&mut self, country: CountryCode) -> Result<(), MarketError> {
        if self.covers(country) {
            return Err(MarketError::DuplicateCountry(country));
        }

        self.countries.push(country);
        self.changed("countries");
        Ok(())
    }

    pub fn remove_country(&mut self, country: CountryCode) -> Result<(), MarketError> {
        if !self.covers(country) {
            return Err(MarketError::CountryNotInMarket(country));
        }

        if self.countries.len() == 1 {
            return Err(MarketError::LastCountry);
        }

        self.countries.retain(|c| *c != country);
        self.changed("countries");
        Ok(())
    }

    pub fn id(&self) -> MarketId {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn countries(&self) -> &[CountryCode] {
        &self.countries
    }

    pub fn currency(&self) -> Currency {
        self.currency
    }

    pub fn prices_include_tax(&self) -> bool {
        self.prices_include_tax
    }

    pub fn languages(&self) -> &[Language] {
        &self.languages
    }

    /// What shoppers are offered here, out of what the store offers (`store_languages`,
    /// its default first): the market's languages in order, or the store's default when
    /// it lists none. A language the store stopped offering since (the store's check and
    /// a market's write don't share a transaction) is left out rather than promised.
    pub fn offered_languages(&self, store_languages: &[Language]) -> Vec<Language> {
        offered_languages(&self.languages, store_languages)
    }
}

/// What a market listing `listed` offers shoppers, out of what the store offers
/// (`servable`, its default first): the listed languages in order that the store still
/// offers, or the store's default when none is left.
pub fn offered_languages(listed: &[Language], servable: &[Language]) -> Vec<Language> {
    let offered: Vec<Language> = listed
        .iter()
        .filter(|l| servable.contains(l))
        .cloned()
        .collect();
    if offered.is_empty() {
        return servable.first().cloned().into_iter().collect();
    }
    offered
}

/// Plain-data form of a `Market` for storage adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarketSnapshot {
    pub id: MarketId,
    pub version: Version,
    pub stamps: Option<Stamps>,
    pub name: String,
    pub countries: Vec<CountryCode>,
    pub currency: Currency,
    pub prices_include_tax: bool,
    pub languages: Vec<Language>,
}

impl Market {
    pub fn snapshot(&self) -> MarketSnapshot {
        MarketSnapshot {
            id: self.id,
            version: self.version,
            stamps: self.stamps,
            name: self.name.clone(),
            countries: self.countries.clone(),
            currency: self.currency,
            prices_include_tax: self.prices_include_tax,
            languages: self.languages.clone(),
        }
    }

    /// Rebuilds a market from storage through `Market::new`, so its rules still hold.
    pub fn restore(s: MarketSnapshot) -> Result<Market, MarketError> {
        let mut market = Market::new(s.id, s.name, s.countries, s.currency)?;
        market.prices_include_tax = s.prices_include_tax;
        market.set_languages(s.languages)?;
        market.version = s.version;
        market.stamps = s.stamps;
        // Built through `new`, which records a change that didn't happen.
        market.changes = Changes::default();
        Ok(market)
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;

    fn cc(s: &str) -> CountryCode {
        CountryCode::new(s).unwrap()
    }

    fn mid(n: u128) -> MarketId {
        MarketId::from_uuid(Uuid::from_u128(n))
    }

    fn market(n: u128, countries: &[&str]) -> Result<Market, MarketError> {
        Market::new(
            mid(n),
            "Test".to_string(),
            countries.iter().map(|s| cc(s)).collect(),
            Currency::EUR,
        )
    }

    #[test]
    fn a_market_needs_a_name() {
        let unnamed = Market::new(mid(1), " ".to_string(), vec![cc("HR")], Currency::EUR);
        assert_eq!(
            unnamed,
            Err(MarketError::Text(hypha_core::TextError::Missing("name")))
        );
    }

    #[test]
    fn new_accepts_countries() {
        let m = market(1, &["RS", "HR"]).unwrap();
        assert_eq!(m.countries(), &[cc("RS"), cc("HR")]);
    }

    #[test]
    fn new_rejects_no_countries() {
        assert_eq!(market(1, &[]), Err(MarketError::NoCountries));
    }

    #[test]
    fn new_rejects_duplicate_country() {
        assert_eq!(
            market(1, &["RS", "RS"]),
            Err(MarketError::DuplicateCountry(cc("RS")))
        );
    }

    #[test]
    fn covers_only_listed_countries() {
        let m = market(1, &["RS"]).unwrap();
        assert!(m.covers(cc("RS")));
        assert!(!m.covers(cc("HR")));
    }

    #[test]
    fn remove_last_country_fails() {
        let mut m = market(1, &["RS"]).unwrap();
        assert_eq!(m.remove_country(cc("RS")), Err(MarketError::LastCountry));
        assert_eq!(m.countries(), &[cc("RS")]);
    }

    #[test]
    fn remove_missing_country_reports_missing_not_last() {
        let mut m = market(1, &["RS"]).unwrap();
        assert_eq!(
            m.remove_country(cc("HR")),
            Err(MarketError::CountryNotInMarket(cc("HR")))
        );
    }

    #[test]
    fn add_country_adds_new_and_rejects_duplicate() {
        let mut m = market(1, &["RS"]).unwrap();
        assert_eq!(m.add_country(cc("HR")), Ok(()));
        assert_eq!(
            m.add_country(cc("RS")),
            Err(MarketError::DuplicateCountry(cc("RS")))
        );
    }

    #[test]
    fn remove_country_removes_it() {
        let mut m = market(1, &["RS", "HR"]).unwrap();
        assert_eq!(m.remove_country(cc("HR")), Ok(()));
        assert!(m.countries.len() == 1);
    }

    fn lang(tag: &str) -> Language {
        Language::new(tag).unwrap()
    }

    fn fields(m: &mut Market) -> Vec<String> {
        m.take_events()
            .iter()
            .flat_map(|e| e.data["fields"].as_array().cloned().unwrap_or_default())
            .filter_map(|f| f.as_str().map(str::to_string))
            .collect()
    }

    #[test]
    fn a_new_market_lists_no_languages() {
        assert_eq!(market(1, &["RS"]).unwrap().languages(), &[]);
    }

    #[test]
    fn languages_keep_their_order_and_are_recorded_as_a_change() {
        let mut m = market(1, &["RS"]).unwrap();
        m.take_events();
        assert_eq!(m.set_languages(vec![lang("sr-Latn"), lang("en")]), Ok(()));
        assert_eq!(m.languages(), &[lang("sr-Latn"), lang("en")]);
        assert_eq!(fields(&mut m), vec!["languages"]);

        // The same list again is no change; another order is one.
        assert_eq!(m.set_languages(vec![lang("sr-Latn"), lang("en")]), Ok(()));
        assert_eq!(fields(&mut m), Vec::<String>::new());
        assert_eq!(m.set_languages(vec![lang("en"), lang("sr-Latn")]), Ok(()));
        assert_eq!(fields(&mut m), vec!["languages"]);
    }

    #[test]
    fn a_language_is_listed_once_whatever_its_spelling() {
        let mut m = market(1, &["RS"]).unwrap();
        assert_eq!(
            m.set_languages(vec![lang("en"), lang("de"), lang("EN")]),
            Err(MarketError::DuplicateLanguage(lang("en")))
        );
        assert_eq!(m.languages(), &[]);
    }

    #[test]
    fn a_market_offers_what_it_lists_that_the_store_offers() {
        assert_eq!(
            offered_languages(
                &[lang("de"), lang("fr"), lang("en")],
                &[lang("en"), lang("de")]
            ),
            vec![lang("de"), lang("en")]
        );
    }

    #[test]
    fn a_market_listing_nothing_the_store_offers_offers_the_stores_default() {
        let store = [lang("en"), lang("de")];
        assert_eq!(offered_languages(&[lang("fr")], &store), vec![lang("en")]);
        assert_eq!(offered_languages(&[], &store), vec![lang("en")]);
        assert!(offered_languages(&[], &[]).is_empty());
    }

    #[test]
    fn shoppers_are_offered_the_markets_languages_the_store_still_offers() {
        let store = [lang("en"), lang("de"), lang("fr")];
        let mut m = market(1, &["RS"]).unwrap();
        assert_eq!(m.offered_languages(&store), vec![lang("en")], "none listed");
        m.set_languages(vec![lang("fr"), lang("hr"), lang("en")])
            .unwrap();
        assert_eq!(m.offered_languages(&store), vec![lang("fr"), lang("en")]);
        m.set_languages(vec![lang("hr")]).unwrap();
        assert_eq!(
            m.offered_languages(&store),
            vec![lang("en")],
            "none left: the store's default"
        );
    }

    #[test]
    fn restore_rejects_a_repeated_language() {
        let mut s = market(1, &["RS"]).unwrap().snapshot();
        s.languages = vec![lang("en"), lang("en")];
        assert_eq!(
            Market::restore(s),
            Err(MarketError::DuplicateLanguage(lang("en")))
        );
    }

    #[test]
    fn restore_round_trips() {
        let mut m = market(1, &["RS", "HR"]).unwrap();
        m.set_prices_include_tax(true);
        m.set_languages(vec![lang("sr-Latn"), lang("en")]).unwrap();
        m.set_version(Version::new(3));
        assert_eq!(Market::restore(m.snapshot()), Ok(m));
    }

    #[test]
    fn restore_rejects_duplicate_country() {
        let mut s = market(1, &["RS"]).unwrap().snapshot();
        s.countries.push(cc("RS"));
        assert_eq!(
            Market::restore(s),
            Err(MarketError::DuplicateCountry(cc("RS")))
        );
    }

    #[test]
    fn prices_exclude_tax_until_set() {
        let mut m = market(1, &["RS"]).unwrap();
        assert!(!m.prices_include_tax());
        m.set_prices_include_tax(true);
        assert!(m.prices_include_tax());
    }
}
