//! The language a storefront read is answered in: which one the request gets, what its
//! overlay may read, and the response that names it. The one place that decides it, for
//! every read that takes `language`.

use axum::{
    Json,
    http::{HeaderValue, header},
    response::{IntoResponse, Response},
};
use hypha_core::{Language, RepoError};
use hypha_markets::Market;
use hypha_stores::Store;
use hypha_translations::{HasTranslations, Overlay, ResourceRef};
use serde::{Deserialize, Serialize};

use crate::{
    error::ApiError,
    state::{AppState, Storage},
};

/// `language` on a read that takes nothing else from its query string.
#[derive(Deserialize, utoipa::IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct LanguageParams {
    /// The language to answer in (a BCP 47 tag, `de`, `pt-BR`): the store's default or
    /// one it has published, else the store's default answers. `Content-Language` names
    /// the one served. Text without a translation stays in the store's default language.
    pub(crate) language: Option<String>,
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

    /// `of` on plain values: `source` is the store's default language, `servable` what
    /// the store offers and `offered` what this read may answer in, its default first.
    fn resolve(
        asked: Option<&str>,
        source: &Language,
        servable: Vec<Language>,
        offered: &[Language],
    ) -> Self {
        let language = asked
            .and_then(|tag| Language::new(tag).ok())
            .filter(|language| offered.contains(language))
            .or_else(|| offered.first().cloned())
            .unwrap_or_else(|| source.clone());
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
        // A tag is letters, digits and hyphens: always a header value.
        if let Ok(language) = HeaderValue::from_str(self.language.as_str()) {
            response
                .headers_mut()
                .insert(header::CONTENT_LANGUAGE, language);
        }
        response
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
    fn a_regional_tag_is_not_resolved_to_its_language() {
        // Falling back from `de-AT` to `de` is each field's, within the language
        // served: asking for a tag that isn't offered gets the default, not a guess.
        assert_eq!(
            in_market(Some("de-AT"), &["en", "de"]).language(),
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
