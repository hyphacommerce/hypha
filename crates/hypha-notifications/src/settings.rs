//! The store's notification settings (design note 0028, §2, NT4): branding every email
//! shares, the sender's name and reply-to, who gets the staff's new-order email, and
//! where order links point. One per store; none stored until the merchant first saves.

use std::fmt;

use hypha_core::{AssetId, Email, OrderId, Stamped, Stamps, Version, Versioned, optional_text};

use crate::SettingsError;

/// A colour as emails write it: `#rrggbb`, lowercase. Email clients don't all read
/// shorthand (`#abc`) or named colours, so only the full form is accepted.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Colour(String);

impl Colour {
    pub fn new(text: &str) -> Result<Self, SettingsError> {
        let invalid = || SettingsError::InvalidColour(text.to_string());
        let hex = text.trim().strip_prefix('#').ok_or_else(invalid)?;
        if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(invalid());
        }
        Ok(Self(format!("#{}", hex.to_ascii_lowercase())))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Colour {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Where an order's page is, as a pattern (design note 0028, §2): an http(s) URL with
/// `{order}` (the order's ID) and optionally `{token}` (its access token, so the link
/// works without the shopper's cookie). The API stays URL-agnostic: the storefront's
/// deployment sets it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OrderUrl(String);

impl OrderUrl {
    pub const ORDER: &str = "{order}";
    pub const TOKEN: &str = "{token}";
    pub const MAX_LEN: usize = 2048;

    pub fn new(text: &str) -> Result<Self, SettingsError> {
        let url = text.trim();
        let invalid = || SettingsError::InvalidOrderUrl(text.to_string());
        let rest = url
            .strip_prefix("https://")
            .or_else(|| url.strip_prefix("http://"))
            .ok_or_else(invalid)?;
        let host = rest.split(['/', '?', '#']).next().unwrap_or("");
        if url.len() > Self::MAX_LEN
            || host.is_empty()
            || host.contains('{')
            || url.chars().any(|c| c.is_whitespace() || c.is_control())
            || !url.contains(Self::ORDER)
        {
            return Err(invalid());
        }
        // Braces only as the two placeholders: anything else is a typo that would end
        // up in every email.
        let bare = url.replace(Self::ORDER, "").replace(Self::TOKEN, "");
        if bare.contains(['{', '}']) {
            return Err(invalid());
        }
        Ok(Self(url.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The link to `order`'s page, with its access token where the pattern asks.
    pub fn link(&self, order: OrderId, token: &str) -> String {
        self.0
            .replace(Self::ORDER, &order.to_string())
            .replace(Self::TOKEN, token)
    }

    /// The scheme and host (with any port): the shop's address in emails (`shop.url`).
    pub fn origin(&self) -> &str {
        let after_scheme = self.0.find("://").map_or(0, |i| i + 3);
        let end = self.0[after_scheme..]
            .find(['/', '?', '#'])
            .map_or(self.0.len(), |i| after_scheme + i);
        &self.0[..end]
    }
}

impl fmt::Display for OrderUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The store's notification settings. `Default` is what a store has until it saves.
///
/// Invariants, checked by every setter and `restore`: the footer within
/// `MAX_FOOTER_LEN` characters, the sender's name by the text rule (trimmed, at most 255
/// characters, blank is none), at most `MAX_STAFF_RECIPIENTS` recipients, each once (in
/// the order given).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationSettings {
    version: Version,
    stamps: Option<Stamps>,
    /// An image from the media library; its asset usage keeps it from being deleted
    /// (written with the settings by the workflow, `hypha-mail`).
    logo: Option<AssetId>,
    accent: Colour,
    /// Plain text under every email: the address, legal lines.
    footer: Option<String>,
    /// `None`: the store's name.
    sender_name: Option<String>,
    reply_to: Option<Email>,
    /// Who gets `staff_new_order`.
    staff_recipients: Vec<Email>,
    order_url: Option<OrderUrl>,
}

impl Versioned for NotificationSettings {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

impl Stamped for NotificationSettings {
    fn stamps(&self) -> Option<Stamps> {
        self.stamps
    }

    fn set_stamps(&mut self, stamps: Stamps) {
        self.stamps = Some(stamps);
    }
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            version: Version::NEW,
            stamps: None,
            logo: None,
            accent: Colour(Self::DEFAULT_ACCENT.to_string()),
            footer: None,
            sender_name: None,
            reply_to: None,
            staff_recipients: Vec::new(),
            order_url: None,
        }
    }
}

impl NotificationSettings {
    /// Near-black: reads on any brand until the merchant picks their own.
    pub const DEFAULT_ACCENT: &str = "#111827";
    pub const MAX_STAFF_RECIPIENTS: usize = 10;
    pub const MAX_FOOTER_LEN: usize = 1000;

    pub fn set_logo(&mut self, logo: Option<AssetId>) {
        self.logo = logo;
    }

    pub fn set_accent(&mut self, accent: Colour) {
        self.accent = accent;
    }

    /// Trimmed; blank clears it.
    pub fn set_footer(&mut self, footer: Option<String>) -> Result<(), SettingsError> {
        self.footer = footer_text(footer)?;
        Ok(())
    }

    /// Trimmed; blank clears it (the store's name is used).
    pub fn set_sender_name(&mut self, name: Option<String>) -> Result<(), SettingsError> {
        self.sender_name = optional_text("sender name", name)?;
        Ok(())
    }

    pub fn set_reply_to(&mut self, reply_to: Option<Email>) {
        self.reply_to = reply_to;
    }

    /// Repeats are dropped, keeping the first; more than `MAX_STAFF_RECIPIENTS` is
    /// refused.
    pub fn set_staff_recipients(&mut self, recipients: Vec<Email>) -> Result<(), SettingsError> {
        self.staff_recipients = recipient_list(recipients)?;
        Ok(())
    }

    pub fn set_order_url(&mut self, order_url: Option<OrderUrl>) {
        self.order_url = order_url;
    }

    pub fn logo(&self) -> Option<AssetId> {
        self.logo
    }

    pub fn accent(&self) -> &Colour {
        &self.accent
    }

    pub fn footer(&self) -> Option<&str> {
        self.footer.as_deref()
    }

    pub fn sender_name(&self) -> Option<&str> {
        self.sender_name.as_deref()
    }

    pub fn reply_to(&self) -> Option<&Email> {
        self.reply_to.as_ref()
    }

    pub fn staff_recipients(&self) -> &[Email] {
        &self.staff_recipients
    }

    pub fn order_url(&self) -> Option<&OrderUrl> {
        self.order_url.as_ref()
    }
}

fn footer_text(footer: Option<String>) -> Result<Option<String>, SettingsError> {
    let Some(footer) = footer else {
        return Ok(None);
    };
    let footer = footer.trim();
    if footer.chars().count() > NotificationSettings::MAX_FOOTER_LEN {
        return Err(SettingsError::FooterTooLong {
            max: NotificationSettings::MAX_FOOTER_LEN,
        });
    }
    Ok((!footer.is_empty()).then(|| footer.to_string()))
}

fn recipient_list(recipients: Vec<Email>) -> Result<Vec<Email>, SettingsError> {
    let mut kept: Vec<Email> = Vec::with_capacity(recipients.len());
    for email in recipients {
        if !kept.contains(&email) {
            kept.push(email);
        }
    }
    if kept.len() > NotificationSettings::MAX_STAFF_RECIPIENTS {
        return Err(SettingsError::TooManyRecipients {
            max: NotificationSettings::MAX_STAFF_RECIPIENTS,
        });
    }
    Ok(kept)
}

/// Plain-data form of `NotificationSettings` for storage adapters (design note 0001,
/// 3.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationSettingsSnapshot {
    pub version: Version,
    pub stamps: Option<Stamps>,
    pub logo: Option<AssetId>,
    pub accent: Colour,
    pub footer: Option<String>,
    pub sender_name: Option<String>,
    pub reply_to: Option<Email>,
    pub staff_recipients: Vec<Email>,
    pub order_url: Option<OrderUrl>,
}

impl NotificationSettings {
    pub fn snapshot(&self) -> NotificationSettingsSnapshot {
        NotificationSettingsSnapshot {
            version: self.version,
            stamps: self.stamps,
            logo: self.logo,
            accent: self.accent.clone(),
            footer: self.footer.clone(),
            sender_name: self.sender_name.clone(),
            reply_to: self.reply_to.clone(),
            staff_recipients: self.staff_recipients.clone(),
            order_url: self.order_url.clone(),
        }
    }

    /// Rebuilds the settings from storage, checking every invariant.
    pub fn restore(s: NotificationSettingsSnapshot) -> Result<Self, SettingsError> {
        Ok(Self {
            version: s.version,
            stamps: s.stamps,
            logo: s.logo,
            accent: s.accent,
            footer: footer_text(s.footer)?,
            sender_name: optional_text("sender name", s.sender_name)?,
            reply_to: s.reply_to,
            staff_recipients: recipient_list(s.staff_recipients)?,
            order_url: s.order_url,
        })
    }
}

#[cfg(test)]
mod tests {
    use hypha_core::TextError;
    use uuid::Uuid;

    use super::*;

    fn email(s: &str) -> Email {
        Email::new(s).unwrap()
    }

    #[test]
    fn a_colour_is_six_hex_digits_kept_lowercase() {
        assert_eq!(
            Colour::new(" #A1b2C3 ").map(|c| c.to_string()),
            Ok("#a1b2c3".to_string())
        );
        for bad in ["a1b2c3", "#abc", "#a1b2c3d", "#g1b2c3", "red", ""] {
            assert_eq!(
                Colour::new(bad),
                Err(SettingsError::InvalidColour(bad.to_string())),
                "{bad}"
            );
        }
    }

    #[test]
    fn an_order_url_is_http_with_the_order_placeholder() {
        assert!(OrderUrl::new("https://shop.example.com/order/{order}?token={token}").is_ok());
        assert!(OrderUrl::new("http://localhost:3002/order/{order}").is_ok());
        for bad in [
            "https://shop.example.com/order",
            "ftp://shop.example.com/{order}",
            "shop.example.com/{order}",
            "https:///{order}",
            "https://{order}.example.com/",
            "https://shop.example.com/{order}/{oder}",
            "https://shop.example.com/{order} x",
        ] {
            assert_eq!(
                OrderUrl::new(bad),
                Err(SettingsError::InvalidOrderUrl(bad.to_string())),
                "{bad}"
            );
        }
    }

    #[test]
    fn an_order_link_fills_both_placeholders() {
        let url = OrderUrl::new("https://shop.example.com/order/{order}?token={token}").unwrap();
        let order = OrderId::from_uuid(Uuid::from_u128(1));
        assert_eq!(
            url.link(order, "tok"),
            format!("https://shop.example.com/order/{order}?token=tok")
        );
    }

    #[test]
    fn the_origin_is_scheme_host_and_port() {
        let origin = |s: &str| OrderUrl::new(s).unwrap().origin().to_string();
        assert_eq!(
            origin("https://shop.example.com/order/{order}"),
            "https://shop.example.com"
        );
        assert_eq!(
            origin("http://localhost:3002?o={order}"),
            "http://localhost:3002"
        );
    }

    #[test]
    fn recipients_are_a_list_without_repeats_up_to_ten() {
        let mut s = NotificationSettings::default();
        s.set_staff_recipients(vec![email("b@x.com"), email("a@x.com"), email("B@x.com")])
            .unwrap();
        assert_eq!(s.staff_recipients(), [email("b@x.com"), email("a@x.com")]);
        let ten: Vec<Email> = (0..10).map(|n| email(&format!("{n}@x.com"))).collect();
        assert_eq!(s.set_staff_recipients(ten.clone()), Ok(()));
        let mut eleven = ten;
        eleven.push(email("more@x.com"));
        assert_eq!(
            s.set_staff_recipients(eleven),
            Err(SettingsError::TooManyRecipients { max: 10 })
        );
    }

    #[test]
    fn text_is_trimmed_and_blank_is_none() {
        let mut s = NotificationSettings::default();
        s.set_footer(Some("  Main St 1 ".to_string())).unwrap();
        s.set_sender_name(Some(" ".to_string())).unwrap();
        assert_eq!(s.footer(), Some("Main St 1"));
        assert_eq!(s.sender_name(), None);
        assert_eq!(
            s.set_footer(Some("x".repeat(1001))),
            Err(SettingsError::FooterTooLong { max: 1000 })
        );
        assert_eq!(
            s.set_sender_name(Some("x".repeat(256))),
            Err(SettingsError::Text(TextError::TooLong("sender name")))
        );
    }

    #[test]
    fn restore_round_trips() {
        let mut s = NotificationSettings::default();
        s.set_logo(Some(AssetId::from_uuid(Uuid::from_u128(5))));
        s.set_accent(Colour::new("#ff0000").unwrap());
        s.set_reply_to(Some(email("help@shop.com")));
        s.set_order_url(Some(OrderUrl::new("https://s.com/{order}").unwrap()));
        assert_eq!(NotificationSettings::restore(s.snapshot()), Ok(s));
    }
}
