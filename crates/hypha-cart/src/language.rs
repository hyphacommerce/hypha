//! The language a cart is read in: one rule for its responses, the order it's placed as
//! and what apps asked about it are told.

use hypha_core::{Language, offered_languages};

use crate::PricedCart;

/// The store's content languages, as plain values from the host: what a cart's language
/// is resolved against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentLanguages {
    /// The store's default: what content is written in.
    pub source: Language,
    /// What shoppers may be served: the default, then the published others.
    pub servable: Vec<Language>,
}

impl ContentLanguages {
    /// A store with its default language only.
    pub fn only(source: Language) -> Self {
        Self {
            servable: vec![source.clone()],
            source,
        }
    }

    /// The language `cart` is read in: the one it was given, held to what its market
    /// offers out of the store's (`Language::pick`), else the store's default. A choice
    /// the market or store stopped offering falls back rather than fails.
    pub fn of(&self, cart: &PricedCart) -> Language {
        self.of_choice(cart.language.as_ref(), &cart.market_languages)
    }

    /// `of` for a read that hasn't priced the cart: its choice (`Cart::language`) and its
    /// market's languages (a checked-out cart's: its order's language alone, as
    /// `PricedCart::market_languages` holds then).
    pub fn of_choice(
        &self,
        language: Option<&Language>,
        market_languages: &[Language],
    ) -> Language {
        let offered = offered_languages(market_languages, &self.servable);
        Language::pick(language, &offered).unwrap_or_else(|| self.source.clone())
    }

    /// What apps asked about `cart` (`checkout.validate`, `shipping.rates`) are told: `of`,
    /// absent where its market names no language the store serves (it reads in the
    /// store's default then). A validation app's answer is kept by a digest of what it
    /// was sent, so this stays absent for a market that names none.
    pub fn told(&self, cart: &PricedCart) -> Option<Language> {
        cart.market_languages
            .iter()
            .any(|l| self.servable.contains(l))
            .then(|| self.of(cart))
    }
}

#[cfg(test)]
mod tests {
    use hypha_core::{CartId, ChannelId, CountryCode, Currency, MarketId, Money};
    use uuid::Uuid;

    use super::*;

    fn lang(tag: &str) -> Language {
        Language::new(tag).unwrap()
    }

    fn cart(language: Option<&str>, market: &[&str]) -> PricedCart {
        let eur = Money::zero(Currency::EUR);
        PricedCart {
            id: CartId::from_uuid(Uuid::from_u128(1)),
            market: MarketId::from_uuid(Uuid::from_u128(2)),
            currency: Currency::EUR,
            country: CountryCode::new("AT").unwrap(),
            channel: ChannelId::from_uuid(Uuid::from_u128(3)),
            email: None,
            customer: None,
            order: None,
            lines: Vec::new(),
            taxes_included: true,
            subtotal: eur,
            shipping_address: None,
            address_check: None,
            app_checks: Vec::new(),
            shipping_groups: Vec::new(),
            shipping_total: eur,
            discounts: Vec::new(),
            discount_total: eur,
            discount_codes: Vec::new(),
            tax_total: eur,
            total: eur,
            payment_method: None,
            marketing_opt_in: false,
            language: language.map(lang),
            market_languages: market.iter().map(|t| lang(t)).collect(),
        }
    }

    fn store() -> ContentLanguages {
        ContentLanguages {
            source: lang("en"),
            servable: vec![lang("en"), lang("de")],
        }
    }

    #[test]
    fn a_cart_is_read_in_its_language_when_its_market_offers_it() {
        assert_eq!(store().of(&cart(Some("de"), &["en", "de"])), lang("de"));
    }

    #[test]
    fn a_cart_without_one_is_read_in_its_markets_default() {
        assert_eq!(store().of(&cart(None, &["de", "en"])), lang("de"));
    }

    #[test]
    fn a_language_no_longer_offered_falls_back_to_the_markets_default() {
        // The market dropped `de` since the cart chose it.
        assert_eq!(store().of(&cart(Some("de"), &["en"])), lang("en"));
        // The store stopped publishing `fr`, which the market still lists.
        assert_eq!(store().of(&cart(Some("fr"), &["fr", "en"])), lang("en"));
    }

    #[test]
    fn a_regional_choice_falls_back_to_its_language() {
        assert_eq!(store().of(&cart(Some("de-AT"), &["en", "de"])), lang("de"));
    }

    #[test]
    fn a_store_with_one_language_reads_every_cart_in_it() {
        let only = ContentLanguages::only(lang("en"));
        assert_eq!(only.of(&cart(Some("de"), &["de"])), lang("en"));
    }

    #[test]
    fn apps_are_told_the_language_the_cart_is_read_in() {
        assert_eq!(
            store().told(&cart(Some("de"), &["en", "de"])),
            Some(lang("de"))
        );
        assert_eq!(store().told(&cart(None, &["en", "de"])), Some(lang("en")));
        // The language the store stopped publishing isn't told, though the market lists
        // it: the cart reads in the market's other one.
        assert_eq!(
            store().told(&cart(Some("fr"), &["fr", "en"])),
            Some(lang("en"))
        );
    }

    #[test]
    fn apps_are_told_none_where_the_market_names_none_the_store_serves() {
        assert_eq!(store().told(&cart(Some("de"), &[])), None);
        assert_eq!(store().told(&cart(Some("fr"), &["fr"])), None);
    }
}
