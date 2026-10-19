//! Calls to co-located apps through Service Bindings (design note 0029, §9). `hypha
//! deploy` runs a first-party app (Stripe, Meilisearch) as its own Worker next to the
//! API, binds the API to it and names the pair in `HYPHA_APP_BINDINGS`
//! (`stripe.example.com=APP_STRIPE,search.example.com=APP_MEILISEARCH`). A call to a URL
//! on one of those hosts (an extension point, a webhook, a block, the proxy) then goes
//! through the binding instead of the public internet: no request leaves Cloudflare's
//! network, and none is billed as a subrequest to the internet.
//!
//! Only the transport changes: the call is checked and signed exactly as before
//! (`outbound::prepare`, `Hypha-Signature`), its URL (and so the `Host` the app sees) is
//! the app's public one, and a host with no binding goes out by `fetch` as any app's.
//! Portable, so its tests run natively.

use url::Url;

/// The variable `hypha deploy` writes the pairs in.
pub const VAR: &str = "HYPHA_APP_BINDINGS";

/// Which host's calls go through which binding.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppBindings(Vec<(String, String)>);

impl AppBindings {
    /// Reads `host=BINDING` pairs, separated by commas (blanks around them ignored). A
    /// host is a lowercase domain name; a binding, a Worker binding's name (`A-Z`,
    /// `0-9`, `_`, not starting with a digit). Each host at most once.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut pairs: Vec<(String, String)> = Vec::new();
        for entry in text.split(',').map(str::trim).filter(|e| !e.is_empty()) {
            let Some((host, binding)) = entry.split_once('=') else {
                return Err(format!("{VAR}: {entry:?} isn't host=BINDING"));
            };
            let (host, binding) = (host.trim(), binding.trim());
            if !is_host(host) {
                return Err(format!(
                    "{VAR}: {host:?} isn't a lowercase domain name (no scheme, port or path)"
                ));
            }
            if !is_binding(binding) {
                return Err(format!(
                    "{VAR}: {binding:?} isn't a binding's name (A-Z, 0-9 and _)"
                ));
            }
            if pairs.iter().any(|(h, _)| h == host) {
                return Err(format!("{VAR}: {host} is named twice"));
            }
            pairs.push((host.to_string(), binding.to_string()));
        }
        Ok(Self(pairs))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The binding `url`'s calls go through: when its host is a co-located app's and its
    /// port is the scheme's own (an app's public URL never names another). `None`
    /// otherwise, and for anything that isn't an http(s) URL.
    pub fn binding_for(&self, url: &str) -> Option<&str> {
        let parsed = Url::parse(url).ok()?;
        if !matches!(parsed.scheme(), "https" | "http") || parsed.port().is_some() {
            return None;
        }
        // `Url` lowercases the host; a trailing dot names the same host.
        let host = parsed.host_str()?.trim_end_matches('.');
        self.0
            .iter()
            .find(|(h, _)| h == host)
            .map(|(_, binding)| binding.as_str())
    }
}

fn is_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
}

fn is_binding(name: &str) -> bool {
    name.bytes()
        .next()
        .is_some_and(|b| b.is_ascii_uppercase() || b == b'_')
        && name
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bindings() -> AppBindings {
        AppBindings::parse("stripe.example.com=APP_STRIPE, search.example.com=APP_MEILISEARCH")
            .unwrap()
    }

    #[test]
    fn a_co_located_apps_url_goes_through_its_binding() {
        let b = bindings();
        assert_eq!(
            b.binding_for("https://stripe.example.com/hypha/payments/start"),
            Some("APP_STRIPE")
        );
        assert_eq!(
            b.binding_for("https://search.example.com/hypha/extensions?x=1"),
            Some("APP_MEILISEARCH")
        );
    }

    #[test]
    fn the_host_is_matched_whatever_its_case() {
        assert_eq!(
            bindings().binding_for("https://Stripe.Example.COM/hypha/webhooks"),
            Some("APP_STRIPE")
        );
    }

    #[test]
    fn the_schemes_own_port_written_out_still_matches() {
        // `Url` drops a port that is the scheme's default.
        assert_eq!(
            bindings().binding_for("https://stripe.example.com:443/x"),
            Some("APP_STRIPE")
        );
    }

    #[test]
    fn another_port_goes_out_by_fetch() {
        assert_eq!(
            bindings().binding_for("https://stripe.example.com:8443/x"),
            None
        );
    }

    #[test]
    fn another_host_goes_out_by_fetch() {
        let b = bindings();
        assert_eq!(b.binding_for("https://rates.example.org/quote"), None);
        // A suffix or a prefix of a bound host isn't that host.
        assert_eq!(b.binding_for("https://evil-stripe.example.com/x"), None);
        assert_eq!(b.binding_for("https://stripe.example.com.evil.org/x"), None);
        assert_eq!(b.binding_for("https://a.stripe.example.com/x"), None);
    }

    #[test]
    fn credentials_in_the_url_dont_fool_the_match() {
        // The host a client would connect to is `evil.org`.
        assert_eq!(
            bindings().binding_for("https://stripe.example.com@evil.org/x"),
            None
        );
        assert_eq!(
            bindings().binding_for("https://user:pw@stripe.example.com/x"),
            Some("APP_STRIPE")
        );
    }

    #[test]
    fn what_isnt_an_http_url_has_no_binding() {
        let b = bindings();
        assert_eq!(b.binding_for("not a url"), None);
        assert_eq!(b.binding_for("ftp://stripe.example.com/x"), None);
    }

    #[test]
    fn an_empty_variable_binds_nothing() {
        let b = AppBindings::parse(" ").unwrap();
        assert!(b.is_empty());
        assert_eq!(b.binding_for("https://stripe.example.com/"), None);
    }

    #[test]
    fn an_entry_without_a_binding_is_refused() {
        assert_eq!(
            AppBindings::parse("stripe.example.com"),
            Err(r#"HYPHA_APP_BINDINGS: "stripe.example.com" isn't host=BINDING"#.to_string())
        );
    }

    #[test]
    fn a_host_with_a_scheme_or_port_is_refused() {
        assert_eq!(
            AppBindings::parse("https://stripe.example.com=APP_STRIPE"),
            Err(r#"HYPHA_APP_BINDINGS: "https://stripe.example.com" isn't a lowercase domain name (no scheme, port or path)"#.to_string())
        );
        assert!(AppBindings::parse("stripe.example.com:443=APP_STRIPE").is_err());
        assert!(AppBindings::parse("Stripe.example.com=APP_STRIPE").is_err());
    }

    #[test]
    fn a_binding_name_with_other_characters_is_refused() {
        assert_eq!(
            AppBindings::parse("stripe.example.com=app-stripe"),
            Err(
                r#"HYPHA_APP_BINDINGS: "app-stripe" isn't a binding's name (A-Z, 0-9 and _)"#
                    .to_string()
            )
        );
        assert!(AppBindings::parse("stripe.example.com=1APP").is_err());
        assert!(AppBindings::parse("stripe.example.com=").is_err());
    }

    #[test]
    fn a_host_named_twice_is_refused() {
        assert_eq!(
            AppBindings::parse("a.example.com=A,a.example.com=B"),
            Err("HYPHA_APP_BINDINGS: a.example.com is named twice".to_string())
        );
    }
}
