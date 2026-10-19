//! Rate limits on the routes that need no credentials (design note 0009), and on the
//! storefront routes that call rate apps (design note 0015).
//!
//! `RateLimiter` is the port: one `check` per rule and key. The host gives one
//! (`host::HostLimiter`): `hypha-host-axum` keeps the counts in its process with
//! `governor`, a Workers host uses Cloudflare's Rate Limiting binding (design note 0026,
//! W8).

use std::time::Duration;

use hypha_config::RateLimits;

/// Every limit, with its quota in `quota`: a closed list, so a handler can't invent a
/// limit and all of them are in one place (0009, section 3).
///
/// Strict limits by email are keyed by email *and* address (`pair_key`): keyed by the
/// email alone, one stranger sending a request per refill period would keep a real
/// user locked out for as long as they liked (third review of 0009).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Rule {
    /// `POST /admin/auth/login`, per client address.
    LoginIp,
    /// `POST /admin/auth/login`, per email and address: guesses at one account from one place.
    LoginEmailFromIp,
    /// `POST /admin/auth/login`, per email from anywhere, loose: bounds guessing spread over
    /// many addresses, while locking someone out takes a great many of them.
    LoginEmail,
    /// `POST /admin/auth/login/mfa`, per client address. Per account, the second-factor
    /// lockout already counts (0008).
    MfaIp,
    /// `POST /admin/auth/password-reset`, per client address.
    ResetIp,
    /// `POST /admin/auth/password-reset`, per email and address. No limit by email alone: the
    /// domain sends at most one email per account every few minutes, and a stranger
    /// asking only sends the owner a valid link.
    ResetEmailFromIp,
    /// `POST /admin/auth/password-reset/confirm`, per client address. Its own bucket, so
    /// asking for a few links doesn't block using one.
    ResetConfirmIp,
    /// `POST /admin/invites/accept`, per client address.
    InviteIp,
    /// `GET /storefront/carts/{id}/shipping-options`, `PUT …/shipping/{group_id}` and
    /// `POST …/checkout`, per cart: each may call rate apps for seconds while holding a
    /// database connection, so a flood would starve the pool (0015, section 9). Per cart
    /// rather than per address, since a server-rendered storefront sends every shopper's
    /// requests from one address. Checked after the publishable key, so only callers
    /// of this store count.
    ShippingQuotesCart,
    /// `POST /storefront/carts`, per client address: without it, new carts would get
    /// around the per-cart limit above. A server-rendered storefront passes each
    /// shopper's address in `X-Forwarded-For` and is listed in `HYPHA_TRUSTED_PROXIES`.
    CartCreateIp,
    /// `GET /oauth/authorize`, `POST /oauth/token` and `POST /oauth/revoke`, per client
    /// address (design note 0029): they need no credentials, and token guessing must be
    /// slow.
    OAuthIp,
    /// A spore's public routes (`/spores/<name>/…`, design note 0030, §7), per client
    /// address: one rule for every spore, so a build's spores don't change a Worker's
    /// Rate Limiting bindings.
    SporePublic,
}

impl Rule {
    /// In declaration order: an in-memory limiter can index by `rule as usize`.
    pub const ALL: [Rule; 12] = [
        Rule::LoginIp,
        Rule::LoginEmailFromIp,
        Rule::LoginEmail,
        Rule::MfaIp,
        Rule::ResetIp,
        Rule::ResetEmailFromIp,
        Rule::ResetConfirmIp,
        Rule::InviteIp,
        Rule::ShippingQuotesCart,
        Rule::CartCreateIp,
        Rule::OAuthIp,
        Rule::SporePublic,
    ];

    /// The rule's key under `[rate_limits]` (`login_ip`): a contract with operators,
    /// never renamed.
    pub fn config_key(self) -> &'static str {
        match self {
            Rule::LoginIp => "login_ip",
            Rule::LoginEmailFromIp => "login_email_from_ip",
            Rule::LoginEmail => "login_email",
            Rule::MfaIp => "mfa_ip",
            Rule::ResetIp => "reset_ip",
            Rule::ResetEmailFromIp => "reset_email_from_ip",
            Rule::ResetConfirmIp => "reset_confirm_ip",
            Rule::InviteIp => "invite_ip",
            Rule::ShippingQuotesCart => "shipping_quotes_cart",
            Rule::CartCreateIp => "cart_create_ip",
            Rule::OAuthIp => "oauth_ip",
            Rule::SporePublic => "spore_public",
        }
    }

    /// The Cloudflare Rate Limiting binding that caps this rule across a Worker's
    /// isolates (design note 0026, W8): its configuration key in capitals. A contract
    /// with deployments, like the key: never renamed.
    pub fn binding(self) -> &'static str {
        match self {
            Rule::LoginIp => "RL_LOGIN_IP",
            Rule::LoginEmailFromIp => "RL_LOGIN_EMAIL_FROM_IP",
            Rule::LoginEmail => "RL_LOGIN_EMAIL",
            Rule::MfaIp => "RL_MFA_IP",
            Rule::ResetIp => "RL_RESET_IP",
            Rule::ResetEmailFromIp => "RL_RESET_EMAIL_FROM_IP",
            Rule::ResetConfirmIp => "RL_RESET_CONFIRM_IP",
            Rule::InviteIp => "RL_INVITE_IP",
            Rule::ShippingQuotesCart => "RL_SHIPPING_QUOTES_CART",
            Rule::CartCreateIp => "RL_CART_CREATE_IP",
            Rule::OAuthIp => "RL_OAUTH_IP",
            Rule::SporePublic => "RL_SPORE_PUBLIC",
        }
    }

    /// The binding's place in a deployment's block of `namespace_id`s (`BINDING_SLOTS`
    /// of them; a namespace is where the counts live). Fixed per rule, never reused: a
    /// new rule takes the next free one, so no other rule's counts move.
    pub fn binding_slot(self) -> u32 {
        match self {
            Rule::LoginIp => 0,
            Rule::LoginEmailFromIp => 1,
            Rule::LoginEmail => 2,
            Rule::MfaIp => 3,
            Rule::ResetIp => 4,
            Rule::ResetEmailFromIp => 5,
            Rule::ResetConfirmIp => 6,
            Rule::InviteIp => 7,
            Rule::ShippingQuotesCart => 8,
            Rule::CartCreateIp => 9,
            Rule::OAuthIp => 10,
            Rule::SporePublic => 11,
        }
    }

    /// The binding's limit per `BINDING_WINDOW` under `limits`: the most the quota
    /// allows in any window (its burst, plus one per period that fits), so the binding
    /// never refuses what the quota allows and only catches what isolates miss.
    pub fn binding_limit(self, limits: &RateLimits) -> u32 {
        let (burst, every) = self.quota(limits);
        let more = BINDING_WINDOW.as_millis() / every.as_millis().max(1);
        burst.saturating_add(u32::try_from(more).unwrap_or(u32::MAX))
    }

    /// `(burst, one more every)` under `limits` (`[rate_limits]`, design note 0017; the
    /// defaults are 0009's, section 3).
    pub fn quota(self, limits: &RateLimits) -> (u32, Duration) {
        let quota = match self {
            Rule::LoginIp => limits.login_ip,
            Rule::LoginEmailFromIp => limits.login_email_from_ip,
            Rule::LoginEmail => limits.login_email,
            Rule::MfaIp => limits.mfa_ip,
            Rule::ResetIp => limits.reset_ip,
            Rule::ResetEmailFromIp => limits.reset_email_from_ip,
            Rule::ResetConfirmIp => limits.reset_confirm_ip,
            Rule::InviteIp => limits.invite_ip,
            Rule::ShippingQuotesCart => limits.shipping_quotes_cart,
            Rule::CartCreateIp => limits.cart_create_ip,
            Rule::OAuthIp => limits.oauth_ip,
            Rule::SporePublic => limits.spore_public,
        };
        (quota.burst, quota.per.0)
    }
}

/// A Rate Limiting binding's window: the longer of the two Cloudflare offers (10 or 60
/// seconds).
pub const BINDING_WINDOW: Duration = Duration::from_secs(60);

/// How many `namespace_id`s a deployment's block holds (`Rule::binding_slot`).
pub const BINDING_SLOTS: u32 = 100;

/// The key for a rule on an email and an address together.
pub fn pair_key(email: &hypha_core::Email, ip: &str) -> String {
    format!("{email}|{ip}")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allowed,
    /// Over the limit; the next attempt is allowed after `retry_after`.
    Limited {
        retry_after: Duration,
    },
}

/// `Err(429 rate_limited)` when `key` is over `rule`'s limit: what handlers call.
pub async fn enforce(
    limiter: &impl RateLimiter,
    rule: Rule,
    key: &str,
) -> Result<(), crate::error::ApiError> {
    match limiter.check(rule, key).await {
        Decision::Allowed => Ok(()),
        Decision::Limited { retry_after } => {
            // Debug, not info: a flood would otherwise write a line per refused request.
            tracing::debug!(?rule, "rate limited");
            Err(crate::error::ApiError::rate_limited(retry_after))
        }
    }
}

/// The port. Async, because a shared version waits on the network.
#[allow(async_fn_in_trait)]
pub trait RateLimiter {
    async fn check(&self, rule: Rule, key: &str) -> Decision;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_binding_is_its_rules_key_in_capitals() {
        for rule in Rule::ALL {
            let key = rule.config_key().to_uppercase();
            assert_eq!(rule.binding(), format!("RL_{key}"));
        }
    }

    #[test]
    fn each_rule_has_its_own_binding_slot_in_the_block() {
        let mut slots: Vec<u32> = Rule::ALL.iter().map(|r| r.binding_slot()).collect();
        slots.sort_unstable();
        slots.dedup();
        assert_eq!(slots.len(), Rule::ALL.len());
        assert!(slots.iter().all(|s| *s < BINDING_SLOTS));
    }

    #[test]
    fn a_bindings_limit_is_the_most_a_quota_allows_in_a_minute() {
        let limits = RateLimits::default();
        // 10, then one every 30 s; 10, then every 2 s; 50, then every 72 s; 3 per 5 min.
        assert_eq!(Rule::LoginIp.binding_limit(&limits), 12);
        assert_eq!(Rule::ShippingQuotesCart.binding_limit(&limits), 40);
        assert_eq!(Rule::LoginEmail.binding_limit(&limits), 50);
        assert_eq!(Rule::ResetEmailFromIp.binding_limit(&limits), 3);
        assert_eq!(Rule::LoginEmailFromIp.binding_limit(&limits), 6);
    }

    #[test]
    fn each_rules_key_is_its_field_under_rate_limits() {
        let fields = serde_json::to_value(RateLimits::default()).unwrap();
        let fields: Vec<&str> = fields
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        let mut keys: Vec<&str> = Rule::ALL.iter().map(|r| r.config_key()).collect();
        keys.sort_unstable();
        let mut fields = fields;
        fields.sort_unstable();
        assert_eq!(keys, fields);
    }
}
