//! Marketing consent: per channel (email, SMS), a small state machine with its opt-in
//! level, when it was given and from where. The newest `consented_at` wins, not the newest
//! write, so a delayed sync can't overwrite a newer opt-out.

use hypha_core::{AppId, Timestamp};
use serde::{Deserialize, Serialize};

/// What consent is about: a way to reach the customer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    Email,
    Sms,
}

impl Channel {
    pub fn as_str(self) -> &'static str {
        match self {
            Channel::Email => "email",
            Channel::Sms => "sms",
        }
    }

    pub fn parse(s: &str) -> Option<Channel> {
        match s {
            "email" => Some(Channel::Email),
            "sms" => Some(Channel::Sms),
            _ => None,
        }
    }
}

impl std::fmt::Display for Channel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsentState {
    /// Never asked: the starting state, which no write can set ("never asked" stays
    /// distinct from "said no").
    #[default]
    NotSubscribed,
    /// Asked, awaiting the provider's confirmation (double opt-in).
    Pending,
    Subscribed,
    Unsubscribed,
    /// Set only by erasure.
    Redacted,
}

impl ConsentState {
    pub fn as_str(self) -> &'static str {
        match self {
            ConsentState::NotSubscribed => "not_subscribed",
            ConsentState::Pending => "pending",
            ConsentState::Subscribed => "subscribed",
            ConsentState::Unsubscribed => "unsubscribed",
            ConsentState::Redacted => "redacted",
        }
    }

    pub fn parse(s: &str) -> Option<ConsentState> {
        match s {
            "not_subscribed" => Some(ConsentState::NotSubscribed),
            "pending" => Some(ConsentState::Pending),
            "subscribed" => Some(ConsentState::Subscribed),
            "unsubscribed" => Some(ConsentState::Unsubscribed),
            "redacted" => Some(ConsentState::Redacted),
            _ => None,
        }
    }
}

impl std::fmt::Display for ConsentState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How the consent was given: ticked once (`single`), or confirmed through the
/// provider's email (`confirmed`); `unknown` for an import that didn't say.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OptInLevel {
    Single,
    Confirmed,
    #[default]
    Unknown,
}

impl OptInLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            OptInLevel::Single => "single",
            OptInLevel::Confirmed => "confirmed",
            OptInLevel::Unknown => "unknown",
        }
    }

    pub fn parse(s: &str) -> Option<OptInLevel> {
        match s {
            "single" => Some(OptInLevel::Single),
            "confirmed" => Some(OptInLevel::Confirmed),
            "unknown" => Some(OptInLevel::Unknown),
            _ => None,
        }
    }
}

/// Where a consent came from. It's taken from who's acting, never from input: an app is
/// `app` with its ID, staff and keys are `admin`, checkout is `checkout`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "app")]
pub enum ConsentSource {
    Checkout,
    Storefront,
    Admin,
    Import,
    App(AppId),
    /// Erasure, or the host itself.
    System,
}

impl ConsentSource {
    /// Its stored form: the kind, or `app:<appid_…>`.
    pub fn as_text(self) -> String {
        match self {
            ConsentSource::Checkout => "checkout".to_string(),
            ConsentSource::Storefront => "storefront".to_string(),
            ConsentSource::Admin => "admin".to_string(),
            ConsentSource::Import => "import".to_string(),
            ConsentSource::App(id) => format!("app:{id}"),
            ConsentSource::System => "system".to_string(),
        }
    }

    pub fn parse(s: &str) -> Option<ConsentSource> {
        Some(match s {
            "checkout" => ConsentSource::Checkout,
            "storefront" => ConsentSource::Storefront,
            "admin" => ConsentSource::Admin,
            "import" => ConsentSource::Import,
            "system" => ConsentSource::System,
            other => ConsentSource::App(other.strip_prefix("app:")?.parse().ok()?),
        })
    }
}

/// A channel's consent as stored.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MarketingConsent {
    pub state: ConsentState,
    pub opt_in_level: OptInLevel,
    /// When the customer last said so; none while never asked.
    pub consented_at: Option<Timestamp>,
    pub source: Option<ConsentSource>,
}

/// What a write asks for: the state, how it was given and when (defaulting to now).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConsentChange {
    pub state: ConsentState,
    pub opt_in_level: OptInLevel,
    pub consented_at: Option<Timestamp>,
}

impl MarketingConsent {
    /// Applies `change` from `source` if it's at least as new as what's stored: the
    /// newest `consented_at` wins. A time in the future counts as `now` (a skewed clock
    /// must not lock the channel until then). Returns whether anything changed. The
    /// caller checks the state may be set (`not_subscribed` and `redacted` can't be
    /// asked for).
    pub(crate) fn apply(
        &mut self,
        change: ConsentChange,
        source: ConsentSource,
        now: Timestamp,
    ) -> bool {
        let at = change.consented_at.unwrap_or(now).min(now);
        if self.consented_at.is_some_and(|stored| at < stored) {
            return false;
        }
        let next = MarketingConsent {
            state: change.state,
            opt_in_level: change.opt_in_level,
            consented_at: Some(at),
            source: Some(source),
        };
        if next == *self {
            return false;
        }
        *self = next;
        true
    }

    /// Back to never asked (an email change, Q7).
    pub(crate) fn reset(&mut self) -> bool {
        if *self == MarketingConsent::default() {
            return false;
        }
        *self = MarketingConsent::default();
        true
    }

    /// Erasure: the state says so, nothing else is kept.
    pub(crate) fn redact(&mut self, now: Timestamp) {
        *self = MarketingConsent {
            state: ConsentState::Redacted,
            opt_in_level: OptInLevel::Unknown,
            consented_at: Some(now),
            source: Some(ConsentSource::System),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(secs: i64) -> Timestamp {
        Timestamp::from_unix_micros(secs * 1_000_000)
    }

    fn subscribe(when: Option<i64>) -> ConsentChange {
        ConsentChange {
            state: ConsentState::Subscribed,
            opt_in_level: OptInLevel::Single,
            consented_at: when.map(at),
        }
    }

    #[test]
    fn the_newest_consent_wins_and_older_writes_change_nothing() {
        let mut consent = MarketingConsent::default();
        assert!(consent.apply(subscribe(Some(100)), ConsentSource::Checkout, at(500)));
        assert_eq!(consent.consented_at, Some(at(100)));
        // Yesterday's unsubscribe, replayed by a provider: too old.
        let unsubscribe = ConsentChange {
            state: ConsentState::Unsubscribed,
            opt_in_level: OptInLevel::Unknown,
            consented_at: Some(at(99)),
        };
        assert!(!consent.apply(unsubscribe, ConsentSource::Admin, at(500)));
        assert_eq!(consent.state, ConsentState::Subscribed);
        // The same instant is taken (a correction of the level, say).
        let confirmed = ConsentChange {
            opt_in_level: OptInLevel::Confirmed,
            ..subscribe(Some(100))
        };
        assert!(consent.apply(confirmed, ConsentSource::Admin, at(500)));
        assert_eq!(consent.opt_in_level, OptInLevel::Confirmed);
        assert_eq!(consent.source, Some(ConsentSource::Admin));
    }

    #[test]
    fn a_time_in_the_future_counts_as_now() {
        let mut consent = MarketingConsent::default();
        assert!(consent.apply(subscribe(Some(9_999)), ConsentSource::Admin, at(7)));
        assert_eq!(consent.consented_at, Some(at(7)));
        // So a later unsubscribe still wins.
        let unsubscribe = ConsentChange {
            state: ConsentState::Unsubscribed,
            opt_in_level: OptInLevel::Unknown,
            consented_at: None,
        };
        assert!(consent.apply(unsubscribe, ConsentSource::Admin, at(8)));
    }

    #[test]
    fn without_a_time_the_write_is_now() {
        let mut consent = MarketingConsent::default();
        assert!(consent.apply(subscribe(None), ConsentSource::Admin, at(7)));
        assert_eq!(consent.consented_at, Some(at(7)));
    }

    #[test]
    fn writing_what_is_stored_changes_nothing() {
        let mut consent = MarketingConsent::default();
        assert!(consent.apply(subscribe(Some(100)), ConsentSource::Admin, at(500)));
        assert!(!consent.apply(subscribe(Some(100)), ConsentSource::Admin, at(600)));
    }

    #[test]
    fn reset_goes_back_to_never_asked() {
        let mut consent = MarketingConsent::default();
        assert!(!consent.reset(), "never asked stays never asked");
        consent.apply(subscribe(Some(100)), ConsentSource::Admin, at(500));
        assert!(consent.reset());
        assert_eq!(consent, MarketingConsent::default());
    }

    #[test]
    fn sources_round_trip_as_text() {
        let app = ConsentSource::App(AppId::from_uuid(uuid::Uuid::from_u128(5)));
        for source in [
            ConsentSource::Checkout,
            ConsentSource::Storefront,
            ConsentSource::Admin,
            ConsentSource::Import,
            ConsentSource::System,
            app,
        ] {
            assert_eq!(ConsentSource::parse(&source.as_text()), Some(source));
        }
        assert_eq!(ConsentSource::parse("app:nope"), None);
        assert_eq!(ConsentSource::parse("fax"), None);
    }
}
