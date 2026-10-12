use std::collections::HashSet;

use hypha_core::{CountryCode, Currency, MarketId, Version, Versioned};

use crate::error::MarketError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Market {
    id: MarketId,
    version: Version,
    name: String,
    countries: Vec<CountryCode>,
    currency: Currency,
}

impl Versioned for Market {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
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

        Ok(Self {
            id,
            version: Version::NEW,
            name,
            countries,
            currency,
        })
    }

    pub fn covers(&self, country: CountryCode) -> bool {
        self.countries.contains(&country)
    }

    pub fn add_country(&mut self, country: CountryCode) -> Result<(), MarketError> {
        if self.covers(country) {
            return Err(MarketError::DuplicateCountry(country));
        }

        self.countries.push(country);
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
}

pub fn market_for(markets: &[Market], country: CountryCode) -> Option<&Market> {
    markets.iter().find(|m| m.covers(country))
}

pub fn check_no_overlap(markets: &[Market]) -> Result<(), MarketError> {
    let mut seen = HashSet::new();
    for market in markets {
        for country in market.countries() {
            if !seen.insert(*country) {
                return Err(MarketError::DuplicateCountry(*country));
            }
        }
    }
    Ok(())
}

/// Plain-data form of a `Market` for storage adapters (design note 0001, 3.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarketSnapshot {
    pub id: MarketId,
    pub version: Version,
    pub name: String,
    pub countries: Vec<CountryCode>,
    pub currency: Currency,
}

impl Market {
    pub fn snapshot(&self) -> MarketSnapshot {
        MarketSnapshot {
            id: self.id,
            version: self.version,
            name: self.name.clone(),
            countries: self.countries.clone(),
            currency: self.currency,
        }
    }

    /// Rebuilds a market from storage through `Market::new`, so its rules still hold.
    pub fn restore(s: MarketSnapshot) -> Result<Market, MarketError> {
        let mut market = Market::new(s.id, s.name, s.countries, s.currency)?;
        market.version = s.version;
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
            Currency::Eur,
        )
    }

    #[test]
    fn new_accepts_countries() {
        let m = market(1, &["RS", "HR"]).unwrap();
        assert_eq!(m.countries(), &[cc("RS"), cc("HR")])
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
        )
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
    fn market_for_finds_the_right_market() {
        let markets = [market(1, &["RS"]).unwrap(), market(2, &["HR"]).unwrap()];
        assert_eq!(market_for(&markets, cc("HR")).map(|m| m.id()), Some(mid(2)));
        assert!(market_for(&markets, cc("DE")).is_none());
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
        assert!(m.countries.len() == 1)
    }

    #[test]
    fn no_overlap_passes_for_disjoint_markets() {
        let markets = [market(1, &["RS"]).unwrap(), market(2, &["HR"]).unwrap()];
        assert_eq!(check_no_overlap(&markets), Ok(()));
    }

    #[test]
    fn no_overlap_rejects_shared_country() {
        let markets = [
            market(1, &["RS", "HR"]).unwrap(),
            market(2, &["HR"]).unwrap(),
        ];
        assert_eq!(
            check_no_overlap(&markets),
            Err(MarketError::DuplicateCountry(cc("HR")))
        );
    }

    #[test]
    fn restore_round_trips() {
        let mut m = market(1, &["RS", "HR"]).unwrap();
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
}
