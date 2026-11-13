//! The language a storefront read is answered in: which one the request gets, what its
//! overlay may read, and the response that names it. The one place that decides it, for
//! every read that takes `language`.

use axum::{
    Json,
    http::{HeaderValue, header},
    response::{IntoResponse, Response},
};
use hypha_cart::PricedCart;
use hypha_checkout::ContentLanguages;
use hypha_core::{CountryCode, Language, RepoError};
use hypha_markets::{HasMarkets, Market, MarketRepo};
use hypha_stores::Store;
use hypha_translations::{HasTranslations, Overlay, ResourceRef};
use serde::{Deserialize, Serialize};

use crate::{
    error::ApiError,
    state::{AppState, Storage},
};

/// What a read that prices nothing takes to choose its language.
#[derive(Deserialize, utoipa::IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct LanguageParams {
    /// The language to answer in (a BCP 47 tag, `de`, `pt-BR`), out of those
    /// `country`'s market offers (`/storefront/countries`): one it doesn't offer is
    /// tried without its region (`de` for `de-AT`), and without it, or with one still
    /// not offered, the market's default answers. Without `country`, out of the
    /// store's default and the languages it has published, the store's default
    /// otherwise. `Content-Language` names the one served. Text without a translation
    /// stays in the store's default language.
    pub(crate) language: Option<String>,
    /// Where the shopper is (ISO 3166-1 alpha-2): its market's languages are the ones
    /// `language` chooses from, as on a product read. Changes nothing else.
    pub(crate) country: Option<String>,
}

/// The language one read is answered in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServedLanguage {
    language: Language,
    /// The store's default, then its published languages: all a shopper may be shown,
    /// as a fallback too.
    servable: Vec<Language>,
    /// Whether `language` is the one the content is written in: nothing to translate.
    source: bool,
}

impl ServedLanguage {
    /// The language a read asking for `asked` gets, in `market` when the read is about
    /// one (it names a country some market sells to):
    ///
    /// - what `asked` names, in the API's spelling, when it's offered: by the market, or
    ///   without one by the store (its default and its published languages);
    /// - else that language without its region (`de` for `de-AT`), when that's offered;
    /// - otherwise (nothing asked, a tag that doesn't parse, a language not offered or
    ///   not published) the market's default, or without a market the store's.
    ///
    /// Never an error: a stale link still renders.
    pub fn of(asked: Option<&str>, store: &Store, market: Option<&Market>) -> Self {
        let servable = store.offered_languages();
        let offered = match market {
            Some(market) => market.offered_languages(&servable),
            None => servable.clone(),
        };
        Self::resolve(asked, store.default_language(), servable, &offered)
    }

    /// The language `cart` is read in: the one it was given, held to what its market
    /// offers now (`ContentLanguages::of`, the rule checkout places its order by), else
    /// the market's default. A checked-out cart (its order's view, whose
    /// `market_languages` hold the order's language alone) is its order's copies, written
    /// in the order's language: that one, whatever the store publishes since.
    pub fn of_cart(cart: &PricedCart, store: &Store) -> Self {
        if cart.order.is_some() {
            return Self::of_order(cart.market_languages.first(), store);
        }
        Self::fixed(content_languages(store).of(cart), store)
    }

    /// `of_cart` for a read that hasn't priced the cart: its choice and its market's
    /// languages (a checked-out cart's: its order's language alone).
    pub fn of_cart_choice(
        language: Option<&Language>,
        market_languages: &[Language],
        store: &Store,
    ) -> Self {
        Self::fixed(
            content_languages(store).of_choice(language, market_languages),
            store,
        )
    }

    /// `language` as it is, for what was written in it already (an order); `None`, an
    /// order from before languages, reads as the store's default.
    pub fn of_order(language: Option<&Language>, store: &Store) -> Self {
        Self::fixed(
            language
                .cloned()
                .unwrap_or_else(|| store.default_language().clone()),
            store,
        )
    }

    fn fixed(language: Language, store: &Store) -> Self {
        Self {
            source: language == *store.default_language(),
            language,
            servable: store.offered_languages(),
        }
    }

    /// `of` on plain values: `source` is the store's default language, `servable` what
    /// the store offers and `offered` what this read may answer in, its default first.
    fn resolve(
        asked: Option<&str>,
        source: &Language,
        servable: Vec<Language>,
        offered: &[Language],
    ) -> Self {
        let asked = asked.and_then(|tag| Language::new(tag).ok());
        let language = Language::pick(asked.as_ref(), offered).unwrap_or_else(|| source.clone());
        Self {
            source: language == *source,
            language,
            servable,
        }
    }

    pub fn language(&self) -> &Language {
        &self.language
    }

    /// Whether the read is answered in the store's default language, as stored.
    pub fn is_source(&self) -> bool {
        self.source
    }

    /// The languages the overlay may read: the store's default and its published ones.
    pub fn servable(&self) -> &[Language] {
        &self.servable
    }

    /// The translations of `resources` into this language; nothing is read for the
    /// store's default.
    pub async fn overlay<P: HasTranslations>(
        &self,
        ports: &P,
        resources: &[ResourceRef],
    ) -> Result<Overlay, RepoError> {
        if self.source {
            return Ok(Overlay::none());
        }
        Overlay::load(ports, &self.language, &self.servable, resources).await
    }

    /// `body` as the response of a read answered in this language.
    pub fn respond<T>(self, body: T) -> Localized<T> {
        Localized {
            language: self.language,
            body,
        }
    }
}

/// The language a read that names its place only by `country` is answered in: in the
/// market selling to it, or by the store's languages without a country or where no
/// market sells. A country code that isn't one is refused, as product reads do.
pub(crate) async fn served_in<S: Storage>(
    state: &AppState<S>,
    ports: &S::Ports<'_>,
    params: &LanguageParams,
) -> Result<ServedLanguage, ApiError> {
    let market = market_in(ports, params).await?;
    served(state, ports, params.language.as_deref(), market.as_ref()).await
}

/// The market selling to `params`' country, for a read that has the store already and
/// passes both to `ServedLanguage::of`; `served_in` reads the store itself.
pub(crate) async fn market_in<P: HasMarkets>(
    ports: &P,
    params: &LanguageParams,
) -> Result<Option<Market>, ApiError> {
    Ok(match params.country.as_deref() {
        Some(country) => {
            let country = CountryCode::new(country)?;
            ports.markets().get_by_country(country).await?
        }
        None => None,
    })
}

/// What checkout resolves a cart's language against: the store's default and the
/// languages it publishes.
pub(crate) fn content_languages(store: &Store) -> ContentLanguages {
    ContentLanguages {
        source: store.default_language().clone(),
        servable: store.offered_languages(),
    }
}

/// The language a shopper's choice is kept as: what `asked` resolves to in `market`
/// (`ServedLanguage::of`: a tag not offered tries its language, then the market's
/// default), or `None` when nothing was asked (the market's default, followed as it
/// changes). Never refuses: a stale or malformed tag is kept as what it reads in.
pub(crate) fn chosen(
    asked: Option<&str>,
    store: &Store,
    market: Option<&Market>,
) -> Option<Language> {
    asked.map(|tag| ServedLanguage::of(Some(tag), store, market).language)
}

/// The language `asked` gets from the store as it is now; see `ServedLanguage::of`.
pub(crate) async fn served<S: Storage>(
    state: &AppState<S>,
    ports: &S::Ports<'_>,
    asked: Option<&str>,
    market: Option<&Market>,
) -> Result<ServedLanguage, ApiError> {
    let store = crate::routes::stores::store_as_system(state, ports).await?;
    Ok(ServedLanguage::of(asked, &store, market))
}

/// A JSON response with `Content-Language`: the language the read resolved to, whether
/// or not each field had a translation.
#[derive(Debug)]
pub struct Localized<T> {
    language: Language,
    body: T,
}

impl<T: Serialize> IntoResponse for Localized<T> {
    fn into_response(self) -> Response {
        let mut response = Json(self.body).into_response();
        name_language(&mut response, &self.language);
        response
    }
}

/// Names `language` in `response`'s `Content-Language`.
pub(crate) fn name_language(response: &mut Response, language: &Language) {
    // A tag is letters, digits and hyphens: always a header value.
    if let Ok(language) = HeaderValue::from_str(language.as_str()) {
        response
            .headers_mut()
            .insert(header::CONTENT_LANGUAGE, language);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lang(tag: &str) -> Language {
        Language::new(tag).unwrap()
    }

    fn langs(tags: &[&str]) -> Vec<Language> {
        tags.iter().map(|tag| lang(tag)).collect()
    }

    /// A store written in English that also publishes German and Austrian German,
    /// read in a market offering `offered`.
    fn in_market(asked: Option<&str>, offered: &[&str]) -> ServedLanguage {
        ServedLanguage::resolve(
            asked,
            &lang("en"),
            langs(&["en", "de", "de-AT"]),
            &langs(offered),
        )
    }

    #[test]
    fn nothing_asked_gets_the_markets_default() {
        let served = in_market(None, &["de", "en"]);
        assert_eq!(served.language(), &lang("de"));
        assert!(!served.is_source());
    }

    #[test]
    fn a_market_whose_default_is_the_stores_answers_the_source() {
        let served = in_market(None, &["en", "de"]);
        assert_eq!(served.language(), &lang("en"));
        assert!(served.is_source());
    }

    #[test]
    fn an_offered_language_is_served() {
        let served = in_market(Some("de-AT"), &["en", "de", "de-AT"]);
        assert_eq!(served.language(), &lang("de-AT"));
        assert!(!served.is_source());
    }

    #[test]
    fn the_stores_default_is_served_where_the_market_offers_it_second() {
        let served = in_market(Some("en"), &["de", "en"]);
        assert_eq!(served.language(), &lang("en"));
        assert!(served.is_source());
    }

    #[test]
    fn the_tag_is_answered_in_the_apis_spelling() {
        assert_eq!(
            in_market(Some("DE-at"), &["en", "de-AT"]).language(),
            &lang("de-AT")
        );
    }

    #[test]
    fn a_language_the_market_does_not_offer_gets_its_default() {
        // Published by the store, but not offered here.
        assert_eq!(
            in_market(Some("de-AT"), &["de", "en"]).language(),
            &lang("de")
        );
    }

    #[test]
    fn a_regional_tag_not_offered_gets_its_language_when_that_is() {
        let served = in_market(Some("de-AT"), &["en", "de"]);
        assert_eq!(served.language(), &lang("de"));
        assert!(!served.is_source());
        let script = ServedLanguage::resolve(
            Some("sr-Latn-RS"),
            &lang("en"),
            langs(&["en", "sr-Latn"]),
            &langs(&["en", "sr-Latn"]),
        );
        assert_eq!(script.language(), &lang("sr-Latn"));
    }

    #[test]
    fn an_offered_regional_tag_wins_over_its_language() {
        assert_eq!(
            in_market(Some("de-AT"), &["en", "de", "de-AT"]).language(),
            &lang("de-AT")
        );
    }

    #[test]
    fn a_regional_tag_whose_language_is_not_offered_gets_the_default() {
        // `de` is published by the store, but this market offers neither.
        assert_eq!(
            in_market(Some("de-AT"), &["en", "fr"]).language(),
            &lang("en")
        );
    }

    #[test]
    fn a_language_is_not_resolved_to_one_of_its_regions() {
        assert_eq!(
            in_market(Some("de"), &["en", "de-AT"]).language(),
            &lang("en")
        );
    }

    #[test]
    fn a_language_the_store_has_not_published_gets_the_default() {
        assert_eq!(in_market(Some("fr"), &["de", "en"]).language(), &lang("de"));
    }

    #[test]
    fn a_tag_that_does_not_parse_gets_the_default() {
        for tag in ["", "german", "de_AT", "de-", "x"] {
            assert_eq!(
                in_market(Some(tag), &["de", "en"]).language(),
                &lang("de"),
                "{tag:?}"
            );
        }
    }

    #[test]
    fn nothing_offered_answers_the_source() {
        let served = in_market(Some("de"), &[]);
        assert_eq!(served.language(), &lang("en"));
        assert!(served.is_source());
    }

    #[test]
    fn the_overlay_may_read_what_the_store_offers_not_what_the_market_does() {
        // `de-AT` served in a market that doesn't offer `de`: its fields still fall
        // back to `de`, which the store publishes.
        let served = in_market(Some("de-AT"), &["en", "de-AT"]);
        assert_eq!(served.servable(), langs(&["en", "de", "de-AT"]));
    }

    #[test]
    fn an_unpublished_parent_is_not_among_what_the_overlay_may_read() {
        let served = ServedLanguage::resolve(
            Some("de-AT"),
            &lang("en"),
            langs(&["en", "de-AT"]),
            &langs(&["en", "de-AT"]),
        );
        assert_eq!(served.language(), &lang("de-AT"));
        assert!(!served.servable().contains(&lang("de")));
    }

    #[test]
    fn the_response_names_the_language() {
        let response = in_market(Some("de"), &["en", "de"])
            .respond(serde_json::json!({}))
            .into_response();
        assert_eq!(response.headers()[header::CONTENT_LANGUAGE], "de");
    }
}
