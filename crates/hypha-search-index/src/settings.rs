//! The merchant's search settings: which options split listings, which facets a storefront
//! shows, and the app that answers searches instead of the built-in engine. One per store.

use std::collections::HashSet;

use hypha_core::{
    AppKey, Event, PendingEvent, Recorded, Recorder, Stamped, Stamps, Subject, TextError, Version,
    Versioned, optional_text, required_text,
};
use serde::Serialize;
use uuid::Uuid;

use crate::{FacetKey, SearchError};

/// A facet a storefront shows, with the merchant's label (else the client labels it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FacetSetting {
    pub key: FacetKey,
    pub label: Option<String>,
}

/// The defaults until the merchant saves: no splits, the default facets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchSettings {
    version: Version,
    stamps: Option<Stamps>,
    /// Option names whose values split listings, in order (compared ignoring case).
    split_by: Vec<String>,
    /// The facets to show, in order; empty means the defaults (`default_facets`).
    facets: Vec<FacetSetting>,
    /// The app answering `/storefront/search` (`search.query`); `None` for the
    /// built-in engine, which also answers whenever the app fails.
    search_app: Option<AppKey>,
    events: Recorder,
}

/// The subject kind of its events; there's one per store, so the subject ID is nil.
pub const SEARCH_SETTINGS: &str = "search_settings";

/// `search_settings.changed`: which of `split_by`, `facets` and `search_app` changed. A
/// changed `split_by` changes every listing key, so search rebuilds every document.
#[derive(Debug, Serialize)]
pub struct SearchSettingsChanged {
    pub fields: Vec<&'static str>,
}

impl Event for SearchSettingsChanged {
    const TYPE: &'static str = "search_settings.changed";
    const VERSION: u16 = 1;
}

impl Default for SearchSettings {
    fn default() -> Self {
        Self {
            version: Version::NEW,
            stamps: None,
            split_by: Vec::new(),
            facets: Vec::new(),
            search_app: None,
            events: Recorder::default(),
        }
    }
}

impl SearchSettings {
    /// Shopify's combined listings split by at most 3 options too.
    pub const MAX_SPLIT: usize = 3;
    pub const MAX_FACETS: usize = 20;

    /// Replaces the split options: each by the text rule, unique ignoring case.
    pub fn set_split_by(&mut self, names: Vec<String>) -> Result<(), SearchError> {
        let mut seen = HashSet::new();
        let mut kept = Vec::with_capacity(names.len());
        for name in names {
            let name = required_text("option name", &name)?;
            if !seen.insert(name.to_lowercase()) {
                return Err(SearchError::DuplicateSplit(name));
            }
            kept.push(name);
        }
        if kept.len() > Self::MAX_SPLIT {
            return Err(SearchError::TooManySplits {
                max: Self::MAX_SPLIT,
            });
        }
        if kept != self.split_by {
            self.split_by = kept;
            self.changed("split_by");
        }
        Ok(())
    }

    /// Replaces the facets shown: unique keys, labels by the text rule.
    pub fn set_facets(&mut self, facets: Vec<FacetSetting>) -> Result<(), SearchError> {
        let mut seen = HashSet::new();
        let mut kept = Vec::with_capacity(facets.len());
        for facet in facets {
            if !seen.insert(facet.key.clone()) {
                return Err(SearchError::DuplicateFacet(facet.key));
            }
            kept.push(FacetSetting {
                label: optional_text("facet label", facet.label)?,
                key: facet.key,
            });
        }
        if kept.len() > Self::MAX_FACETS {
            return Err(SearchError::TooManyFacets {
                max: Self::MAX_FACETS,
            });
        }
        if kept != self.facets {
            self.facets = kept;
            self.changed("facets");
        }
        Ok(())
    }

    /// Names the app that answers searches (`None`: the built-in engine). The caller
    /// checked that it answers `search.query`.
    pub fn set_search_app(&mut self, app: Option<AppKey>) {
        if app != self.search_app {
            self.search_app = app;
            self.changed("search_app");
        }
    }

    fn changed(&mut self, field: &'static str) {
        // One event per write, naming every field: coalesce with a pending one.
        let mut fields: Vec<&'static str> = self
            .events
            .take()
            .iter()
            .filter_map(|e| e.data["fields"].as_array().cloned())
            .flatten()
            .filter_map(|f| match f.as_str() {
                Some("split_by") => Some("split_by"),
                Some("facets") => Some("facets"),
                Some("search_app") => Some("search_app"),
                _ => None,
            })
            .collect();
        if !fields.contains(&field) {
            fields.push(field);
        }
        fields.sort_unstable();
        self.events.record(
            Subject::new(SEARCH_SETTINGS, Uuid::nil()),
            &SearchSettingsChanged { fields },
        );
    }

    pub fn split_by(&self) -> &[String] {
        &self.split_by
    }

    pub fn facets(&self) -> &[FacetSetting] {
        &self.facets
    }

    pub fn search_app(&self) -> Option<&AppKey> {
        self.search_app.as_ref()
    }

    pub fn snapshot(&self) -> SearchSettingsSnapshot {
        SearchSettingsSnapshot {
            version: self.version,
            stamps: self.stamps,
            split_by: self.split_by.clone(),
            facets: self
                .facets
                .iter()
                .map(|f| (f.key.to_string(), f.label.clone()))
                .collect(),
            search_app: self.search_app.as_ref().map(|a| a.as_str().to_string()),
        }
    }

    /// Rebuilds the settings from storage, re-checking every rule.
    pub fn restore(s: SearchSettingsSnapshot) -> Result<Self, SearchError> {
        let mut settings = SearchSettings::default();
        settings.set_split_by(s.split_by)?;
        let facets = s
            .facets
            .into_iter()
            .map(|(key, label)| {
                Ok(FacetSetting {
                    key: key.parse().map_err(SearchError::UnknownFacet)?,
                    label,
                })
            })
            .collect::<Result<Vec<_>, SearchError>>()?;
        settings.set_facets(facets)?;
        settings.search_app = s
            .search_app
            .map(|a| AppKey::new(&a))
            .transpose()
            .map_err(SearchError::SearchApp)?;
        settings.version = s.version;
        settings.stamps = s.stamps;
        settings.events = Recorder::default();
        Ok(settings)
    }
}

/// Plain-data form for storage adapters; facets as their key strings and labels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchSettingsSnapshot {
    pub version: Version,
    pub stamps: Option<Stamps>,
    pub split_by: Vec<String>,
    pub facets: Vec<(String, Option<String>)>,
    /// The search app's key.
    pub search_app: Option<String>,
}

impl Recorded for SearchSettings {
    fn take_events(&mut self) -> Vec<PendingEvent> {
        self.events.take()
    }
}

impl Versioned for SearchSettings {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

impl Stamped for SearchSettings {
    fn stamps(&self) -> Option<Stamps> {
        self.stamps
    }

    fn set_stamps(&mut self, stamps: Stamps) {
        self.stamps = Some(stamps);
    }
}

impl From<TextError> for SearchError {
    fn from(e: TextError) -> Self {
        SearchError::Text(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    #[test]
    fn split_names_are_unique_ignoring_case() {
        let mut s = SearchSettings::default();
        assert_eq!(
            s.set_split_by(names(&["Color", "color"])),
            Err(SearchError::DuplicateSplit("color".to_string()))
        );
        assert_eq!(s.set_split_by(names(&[" Color "])), Ok(()));
        assert_eq!(s.split_by(), ["Color"]);
    }

    #[test]
    fn at_most_three_splits() {
        let mut s = SearchSettings::default();
        assert_eq!(s.set_split_by(names(&["a", "b", "c"])), Ok(()));
        assert_eq!(
            s.set_split_by(names(&["a", "b", "c", "d"])),
            Err(SearchError::TooManySplits { max: 3 })
        );
    }

    #[test]
    fn facets_are_unique() {
        let mut s = SearchSettings::default();
        let vendor = || FacetSetting {
            key: FacetKey::Vendor,
            label: None,
        };
        assert_eq!(
            s.set_facets(vec![vendor(), vendor()]),
            Err(SearchError::DuplicateFacet(FacetKey::Vendor))
        );
    }

    #[test]
    fn one_event_names_every_changed_field() {
        let mut s = SearchSettings::default();
        s.set_split_by(names(&["Color"])).unwrap();
        s.set_facets(vec![FacetSetting {
            key: FacetKey::Tag,
            label: Some("Tags".to_string()),
        }])
        .unwrap();
        let events = s.take_events();
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].data,
            serde_json::json!({ "fields": ["facets", "split_by"] })
        );
    }

    #[test]
    fn setting_the_same_values_records_nothing() {
        let mut s = SearchSettings::default();
        s.set_split_by(vec![]).unwrap();
        s.set_facets(vec![]).unwrap();
        assert!(s.take_events().is_empty());
    }

    #[test]
    fn restore_round_trips() {
        let mut s = SearchSettings::default();
        s.set_split_by(names(&["Color"])).unwrap();
        s.set_facets(vec![FacetSetting {
            key: FacetKey::Option("size".to_string()),
            label: Some("Size".to_string()),
        }])
        .unwrap();
        s.set_version(Version::new(3));
        assert_eq!(SearchSettings::restore(s.snapshot()), Ok(s));
    }
}
