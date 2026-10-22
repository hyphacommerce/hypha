//! The app manifest (`hypha-app.json`): what an app is, the scopes it asks for and why, and
//! what it registers (webhooks, extension points, UI). An app serves it at a URL, which is
//! also its OAuth client ID; Hypha fetches it at `/oauth/authorize` and keeps the copy it
//! installed from.
//!
//! `ManifestDocument` is the JSON as written (its JSON Schema, generated from these
//! types, is published as `docs/apps/manifest.schema.json`); `AppManifest::parse`
//! checks every rule and reports every problem at once, with its path, and resolves
//! relative URLs against `app_url`.

use std::collections::BTreeMap;

use hypha_core::{AppKey, CallbackUrl, ErrorCode, Scope};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::EventPattern;
use crate::custom_fields::{self, AppCustomField, ManifestCustomField};

/// The JSON Schema URL a manifest may name in `$schema`.
pub const SCHEMA_URL: &str = "https://hypha.dev/schemas/app-manifest/1.json";

/// The most a manifest may weigh: plenty for every registration an app has.
pub const MAX_MANIFEST_BYTES: usize = 64 * 1024;

/// The client ID of a private app (a store's own integration, its manifest uploaded rather
/// than served): `urn:hypha:private:<key>`. Not a URL, so neither `/oauth/authorize` nor a
/// refresh can fetch it; such an app gets its tokens when staff approve it, not through a
/// redirect.
pub const PRIVATE_CLIENT_PREFIX: &str = "urn:hypha:private:";

/// The client ID a private app with `key` is installed under.
pub fn private_client_id(key: &AppKey) -> String {
    format!("{PRIVATE_CLIENT_PREFIX}{key}")
}

/// Whether `client_id` (a manifest URL, or a private app's URN) names a private app.
pub fn is_private_client(client_id: &str) -> bool {
    client_id.starts_with(PRIVATE_CLIENT_PREFIX)
}

/// Where a manifest came from, which decides one rule: a served manifest (a public app,
/// installed through OAuth's redirect) lists 1 to 10 `redirect_uris`; an uploaded one
/// (a private app, which gets its tokens on approval) may list none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManifestSource {
    Served,
    Uploaded,
}

impl ManifestSource {
    /// The source of a manifest installed under `client_id`.
    pub fn of_client(client_id: &str) -> Self {
        if is_private_client(client_id) {
            ManifestSource::Uploaded
        } else {
            ManifestSource::Served
        }
    }
}

/// An app manifest as written (`hypha-app.json`). URLs other than `app_url` may be
/// relative: they resolve against `app_url`. Fields not listed here are refused, so a
/// typo is caught rather than ignored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[cfg_attr(test, schemars(title = "Hypha app manifest"))]
#[serde(deny_unknown_fields)]
pub struct ManifestDocument {
    /// `https://hypha.dev/schemas/app-manifest/1.json`, for editors.
    #[serde(rename = "$schema", default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    /// The app's key: `app_` and lowercase letters, digits and underscores, at most 64
    /// characters. The same in every store that installs it.
    pub key: String,
    /// Its name, as staff see it on the consent screen and in the admin.
    pub name: String,
    /// The app's own version (`1.2.0`), shown to staff.
    pub version: String,
    /// Who makes it, shown on the consent screen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,
    /// A square image, shown beside its name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// The app's base URL (`https://`, or `http://localhost` in development): every
    /// relative URL below resolves against it.
    pub app_url: String,
    /// Where Hypha may send the browser back after consent: `/oauth/authorize`'s
    /// `redirect_uri` must be one of these exactly. 1 to 10; none for a private app,
    /// whose manifest is uploaded and which gets its tokens on approval.
    pub redirect_uris: Vec<String>,
    /// The scopes the app asks for (`orders:read`, `customer_data:email`, …).
    #[serde(default)]
    pub scopes: Vec<String>,
    /// Why the app needs a scope, shown on the consent screen. Required for every
    /// `customer_data:*` scope it asks for.
    #[serde(default)]
    pub scope_reasons: BTreeMap<String, String>,
    /// The app's privacy policy, linked from the consent screen.
    pub privacy_url: String,
    /// Where the app's webhooks go and which events it wants.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webhooks: Option<ManifestWebhooks>,
    /// The extension points the app implements (`payments.start` v1, …).
    #[serde(default)]
    pub extension_points: Vec<ManifestExtensionPoint>,
    /// A settings form any admin can render without the app's own page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<ManifestSettings>,
    /// What the app adds to an admin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub admin: Option<ManifestAdmin>,
    /// What the app adds to a storefront.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storefront: Option<ManifestStorefront>,
    /// The custom fields the app keeps on Hypha's objects, in its own namespace
    /// (`app_<name>.<key>`): created when staff approve the app, updated when they approve
    /// a refreshed manifest, deleted when a refreshed manifest no longer declares them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom_fields: Vec<ManifestCustomField>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ManifestWebhooks {
    /// Where Hypha POSTs events, signed with the app's secret.
    pub url: String,
    /// Event types (`order.placed`), every type of a kind (`product.*`) or `*`. 1 to 50.
    pub events: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ManifestExtensionPoint {
    /// The point's name: `payments.start`.
    pub point: String,
    /// The contract's version the app implements, from 1.
    pub version: u16,
    /// Where Hypha calls it.
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ManifestSettings {
    /// A JSON Schema (an object) for the app's settings form.
    #[cfg_attr(test, schemars(with = "serde_json::Map<String, serde_json::Value>"))]
    pub schema: serde_json::Value,
    /// Fields of the form that hold secrets: shown masked.
    #[serde(default)]
    pub secret_fields: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ManifestAdmin {
    /// Full pages under the app (`apps.page`), in an iframe.
    #[serde(default)]
    pub pages: Vec<AdminPage>,
    /// Blocks on admin pages (`order.payment`, …), answered as UI trees.
    #[serde(default)]
    pub blocks: Vec<AdminBlock>,
    /// Buttons (`payment`, …) that call the app.
    #[serde(default)]
    pub actions: Vec<AdminAction>,
    /// The app's endpoint behind `/admin/apps/{app}/proxy/…`: what an admin's settings
    /// form saves to, and anything else an admin calls the app for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct AdminPage {
    /// Lowercase letters, digits, `-` and `_`; unique among the app's pages.
    pub id: String,
    pub title: String,
    pub url: String,
    /// Where an admin lists it (`apps`, `settings`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nav: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct AdminBlock {
    pub id: String,
    /// The slot it fills (`order.payment`): admins show the targets they implement.
    pub target: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct AdminAction {
    pub id: String,
    pub target: String,
    pub label: String,
    /// The extension point the action calls (`payments.capture`); this or `url`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub point: Option<String>,
    /// The app's own URL the action calls; this or `point`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// A JSON Schema (an object) for the action's input: a frontend shows a form made
    /// from it before running the action. The app checks the input it gets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(
        test,
        schemars(with = "Option<serde_json::Map<String, serde_json::Value>>")
    )]
    pub input_schema: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ManifestStorefront {
    /// Web components a storefront may load.
    #[serde(default)]
    pub elements: Vec<StorefrontElement>,
    /// The app's endpoint behind `/storefront/apps/{app}/proxy/…`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct StorefrontElement {
    pub id: String,
    pub target: String,
    /// The ES module that defines the element.
    pub module: String,
    /// The custom element's tag: lowercase, with a `-` (`stripe-payment`).
    pub tag: String,
}

/// One broken rule: where (`redirect_uris[0]`, `scope_reasons`) and what.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestProblem {
    pub path: String,
    pub message: String,
}

/// Why a manifest was refused: every problem found, in document order.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("the app manifest is invalid: {}", describe(.problems))]
pub struct ManifestError {
    pub problems: Vec<ManifestProblem>,
}

fn describe(problems: &[ManifestProblem]) -> String {
    problems
        .iter()
        .map(|p| format!("{}: {}", p.path, p.message))
        .collect::<Vec<_>>()
        .join("; ")
}

impl ErrorCode for ManifestError {
    fn code(&self) -> &'static str {
        "manifest_invalid"
    }
}

impl ManifestError {
    fn one(path: &str, message: impl Into<String>) -> Self {
        Self {
            problems: vec![ManifestProblem {
                path: path.to_string(),
                message: message.into(),
            }],
        }
    }
}

/// A checked manifest: the document with every URL absolute, and its rules' values
/// typed.
#[derive(Debug, Clone, PartialEq)]
pub struct AppManifest {
    document: ManifestDocument,
    key: AppKey,
    app_url: CallbackUrl,
    redirect_uris: Vec<CallbackUrl>,
    /// In `Scope::ALL` order, each once.
    scopes: Vec<Scope>,
    webhook: Option<(CallbackUrl, Vec<EventPattern>)>,
    /// In the document's order.
    custom_fields: Vec<AppCustomField>,
}

impl AppManifest {
    pub const MAX_NAME_LEN: usize = 100;
    pub const MAX_REDIRECT_URIS: usize = 10;
    pub const MAX_EVENTS: usize = 50;
    pub const MAX_REGISTRATIONS: usize = 50;
    pub const MAX_REASON_LEN: usize = 500;

    /// Parses and checks a manifest's bytes (at most `MAX_MANIFEST_BYTES`).
    pub fn parse(bytes: &[u8]) -> Result<Self, ManifestError> {
        Self::parse_from(bytes, ManifestSource::Served)
    }

    /// Parses and checks a manifest's bytes from `source` (an upload may list no
    /// `redirect_uris`).
    pub fn parse_from(bytes: &[u8], source: ManifestSource) -> Result<Self, ManifestError> {
        if bytes.len() > MAX_MANIFEST_BYTES {
            return Err(ManifestError::one(
                "",
                format!("is larger than {MAX_MANIFEST_BYTES} bytes"),
            ));
        }
        let document: ManifestDocument = serde_json::from_slice(bytes)
            .map_err(|e| ManifestError::one("", format!("is not a manifest: {e}")))?;
        Self::check(document, source)
    }

    /// From a stored copy (`AppManifest::to_value`) of a served manifest, with every
    /// rule checked again.
    pub fn from_value(value: serde_json::Value) -> Result<Self, ManifestError> {
        Self::from_value_of(value, ManifestSource::Served)
    }

    /// From a stored copy of a manifest that came from `source`, with every rule
    /// checked again.
    pub fn from_value_of(
        value: serde_json::Value,
        source: ManifestSource,
    ) -> Result<Self, ManifestError> {
        let document: ManifestDocument = serde_json::from_value(value)
            .map_err(|e| ManifestError::one("", format!("is not a manifest: {e}")))?;
        Self::check(document, source)
    }

    /// The checked document as JSON (URLs absolute): what storage keeps and the admin
    /// shows.
    pub fn to_value(&self) -> serde_json::Value {
        // Plain strings, numbers, lists and maps with string keys: can't fail.
        serde_json::to_value(&self.document)
            .unwrap_or_else(|e| unreachable!("a manifest always serializes: {e}"))
    }

    pub fn document(&self) -> &ManifestDocument {
        &self.document
    }
    pub fn key(&self) -> &AppKey {
        &self.key
    }
    pub fn name(&self) -> &str {
        &self.document.name
    }
    pub fn version(&self) -> &str {
        &self.document.version
    }
    pub fn publisher(&self) -> Option<&str> {
        self.document.publisher.as_deref()
    }
    pub fn icon(&self) -> Option<&str> {
        self.document.icon.as_deref()
    }
    pub fn app_url(&self) -> &CallbackUrl {
        &self.app_url
    }
    pub fn privacy_url(&self) -> &str {
        &self.document.privacy_url
    }
    pub fn redirect_uris(&self) -> &[CallbackUrl] {
        &self.redirect_uris
    }
    /// Whether `uri` is one of the redirect URIs, exactly (RFC 6749 §3.1.2.3).
    pub fn allows_redirect(&self, uri: &str) -> bool {
        self.redirect_uris.iter().any(|r| r.as_str() == uri)
    }
    pub fn scopes(&self) -> &[Scope] {
        &self.scopes
    }
    /// The manifest's reason for `scope`, if it gives one.
    pub fn reason(&self, scope: Scope) -> Option<&str> {
        self.document
            .scope_reasons
            .get(scope.as_str())
            .map(String::as_str)
    }
    /// The webhook URL and the events it wants.
    pub fn webhook(&self) -> Option<(&CallbackUrl, &[EventPattern])> {
        self.webhook
            .as_ref()
            .map(|(url, events)| (url, &events[..]))
    }

    /// The custom fields the app declares, checked, in the document's order.
    pub fn custom_fields(&self) -> &[AppCustomField] {
        &self.custom_fields
    }

    fn check(
        mut document: ManifestDocument,
        source: ManifestSource,
    ) -> Result<Self, ManifestError> {
        let mut c = Checker::default();

        let key = AppKey::new_app(&document.key)
            .map_err(|e| c.add("key", e.to_string()))
            .ok();
        c.text("name", &document.name, Self::MAX_NAME_LEN);
        let version = &document.version;
        if version.is_empty()
            || version.len() > 32
            || !version
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '+'))
        {
            c.add(
                "version",
                "is 1 to 32 letters, digits, `.`, `-` or `+` (`1.2.0`)",
            );
        }
        if let Some(publisher) = &document.publisher {
            c.text("publisher", publisher, Self::MAX_NAME_LEN);
        }

        let app_url = match CallbackUrl::new(&document.app_url) {
            Ok(url) => Some(url),
            Err(_) => {
                c.add(
                    "app_url",
                    "is an absolute https URL (or http to localhost), without credentials",
                );
                None
            }
        };
        let base = app_url
            .as_ref()
            .and_then(|u| url::Url::parse(u.as_str()).ok());
        let resolve = |c: &mut Checker, path: &str, raw: &mut String| -> Option<CallbackUrl> {
            let base = base.as_ref()?;
            let resolved = base
                .join(raw.trim())
                .ok()
                .and_then(|u| CallbackUrl::new(u.as_str()).ok());
            match resolved {
                Some(url) => {
                    *raw = url.as_str().to_string();
                    Some(url)
                }
                None => {
                    c.add(
                        path,
                        "is an https URL (or http to localhost), absolute or relative to app_url",
                    );
                    None
                }
            }
        };

        if let Some(icon) = &mut document.icon {
            resolve(&mut c, "icon", icon);
        }
        resolve(&mut c, "privacy_url", &mut document.privacy_url);

        let mut redirect_uris = Vec::new();
        let fewest = match source {
            ManifestSource::Served => 1,
            ManifestSource::Uploaded => 0,
        };
        if document.redirect_uris.len() < fewest
            || document.redirect_uris.len() > Self::MAX_REDIRECT_URIS
        {
            c.add(
                "redirect_uris",
                format!("lists {fewest} to {} URLs", Self::MAX_REDIRECT_URIS),
            );
        }
        for (n, uri) in document.redirect_uris.iter_mut().enumerate() {
            let path = format!("redirect_uris[{n}]");
            if uri.contains('#') {
                c.add(&path, "has no fragment (`#…`)");
                continue;
            }
            if let Some(url) = resolve(&mut c, &path, uri) {
                redirect_uris.push(url);
            }
        }

        let mut scopes: Vec<Scope> = Vec::new();
        for (n, name) in document.scopes.iter().enumerate() {
            match Scope::parse(name) {
                Some(scope) if scopes.contains(&scope) => {
                    c.add(&format!("scopes[{n}]"), format!("lists {name:?} twice"))
                }
                Some(scope) => scopes.push(scope),
                None => c.add(&format!("scopes[{n}]"), format!("{name:?} is not a scope")),
            }
        }
        for (name, reason) in &document.scope_reasons {
            let path = format!("scope_reasons.{name}");
            match Scope::parse(name) {
                Some(scope) if scopes.contains(&scope) => {
                    if reason.trim().is_empty() || reason.chars().count() > Self::MAX_REASON_LEN {
                        c.add(
                            &path,
                            format!("is 1 to {} characters", Self::MAX_REASON_LEN),
                        );
                    }
                }
                _ => c.add(&path, "explains a scope the manifest doesn't ask for"),
            }
        }
        for scope in &scopes {
            if scope.is_personal_data() && !document.scope_reasons.contains_key(scope.as_str()) {
                c.add(
                    "scope_reasons",
                    format!("says why the app needs {scope}: personal data needs a reason"),
                );
            }
        }
        scopes.sort_by_key(|s| Scope::ALL.iter().position(|a| a == s));

        let mut webhook = None;
        if let Some(hooks) = &mut document.webhooks {
            let url = resolve(&mut c, "webhooks.url", &mut hooks.url);
            if hooks.events.is_empty() || hooks.events.len() > Self::MAX_EVENTS {
                c.add(
                    "webhooks.events",
                    format!("lists 1 to {} event types", Self::MAX_EVENTS),
                );
            }
            let mut events: Vec<EventPattern> = Vec::new();
            for (n, event) in hooks.events.iter().enumerate() {
                match EventPattern::new(event.trim()) {
                    Ok(p) if !events.contains(&p) => events.push(p),
                    Ok(_) => {}
                    Err(e) => c.add(&format!("webhooks.events[{n}]"), e.to_string()),
                }
            }
            webhook = url.map(|url| (url, events));
        }

        c.count(
            "extension_points",
            document.extension_points.len(),
            Self::MAX_REGISTRATIONS,
        );
        let mut seen: Vec<(String, u16)> = Vec::new();
        for (n, point) in document.extension_points.iter_mut().enumerate() {
            let path = format!("extension_points[{n}]");
            if !is_point_name(&point.point) {
                c.add(
                    &format!("{path}.point"),
                    "is a dotted lowercase name (`payments.start`)",
                );
            }
            if point.version == 0 {
                c.add(&format!("{path}.version"), "starts at 1");
            }
            if seen.contains(&(point.point.clone(), point.version)) {
                c.add(&path, "lists this point and version twice");
            }
            seen.push((point.point.clone(), point.version));
            resolve(&mut c, &format!("{path}.url"), &mut point.url);
        }

        if let Some(settings) = &document.settings {
            if !settings.schema.is_object() {
                c.add("settings.schema", "is a JSON Schema object");
            }
            c.count(
                "settings.secret_fields",
                settings.secret_fields.len(),
                Self::MAX_REGISTRATIONS,
            );
            for (n, field) in settings.secret_fields.iter().enumerate() {
                if field.trim().is_empty() {
                    c.add(&format!("settings.secret_fields[{n}]"), "is required");
                }
            }
        }

        if let Some(admin) = &mut document.admin {
            c.count("admin.pages", admin.pages.len(), Self::MAX_REGISTRATIONS);
            let mut ids = Vec::new();
            for (n, page) in admin.pages.iter_mut().enumerate() {
                let path = format!("admin.pages[{n}]");
                c.id(&path, &page.id, &mut ids);
                c.text(&format!("{path}.title"), &page.title, Self::MAX_NAME_LEN);
                resolve(&mut c, &format!("{path}.url"), &mut page.url);
            }
            c.count("admin.blocks", admin.blocks.len(), Self::MAX_REGISTRATIONS);
            let mut ids = Vec::new();
            for (n, block) in admin.blocks.iter_mut().enumerate() {
                let path = format!("admin.blocks[{n}]");
                c.id(&path, &block.id, &mut ids);
                c.target(&path, &block.target);
                resolve(&mut c, &format!("{path}.url"), &mut block.url);
            }
            c.count(
                "admin.actions",
                admin.actions.len(),
                Self::MAX_REGISTRATIONS,
            );
            let mut ids = Vec::new();
            for (n, action) in admin.actions.iter_mut().enumerate() {
                let path = format!("admin.actions[{n}]");
                c.id(&path, &action.id, &mut ids);
                c.target(&path, &action.target);
                c.text(&format!("{path}.label"), &action.label, Self::MAX_NAME_LEN);
                match (&action.point, &mut action.url) {
                    (Some(point), None) if is_point_name(point) => {}
                    (Some(_), None) => c.add(
                        &format!("{path}.point"),
                        "is a dotted lowercase name (`payments.capture`)",
                    ),
                    (None, Some(url)) => {
                        resolve(&mut c, &format!("{path}.url"), url);
                    }
                    _ => c.add(&path, "names either a `point` or a `url`"),
                }
                if action.input_schema.as_ref().is_some_and(|s| !s.is_object()) {
                    c.add(&format!("{path}.input_schema"), "is a JSON Schema object");
                }
            }
            if let Some(proxy) = &mut admin.proxy {
                resolve(&mut c, "admin.proxy", proxy);
            }
        }

        if let Some(storefront) = &mut document.storefront {
            c.count(
                "storefront.elements",
                storefront.elements.len(),
                Self::MAX_REGISTRATIONS,
            );
            let mut ids = Vec::new();
            for (n, element) in storefront.elements.iter_mut().enumerate() {
                let path = format!("storefront.elements[{n}]");
                c.id(&path, &element.id, &mut ids);
                c.target(&path, &element.target);
                resolve(&mut c, &format!("{path}.module"), &mut element.module);
                if !is_element_tag(&element.tag) {
                    c.add(
                        &format!("{path}.tag"),
                        "is a custom element name: lowercase, starting with a letter, with a `-`",
                    );
                }
            }
            if let Some(proxy) = &mut storefront.proxy {
                resolve(&mut c, "storefront.proxy", proxy);
            }
        }

        let custom_fields = custom_fields::check(&mut c, &document.custom_fields, &document.scopes);

        if !c.problems.is_empty() {
            return Err(ManifestError {
                problems: c.problems,
            });
        }
        // Without problems, every typed value above was set.
        let (Some(key), Some(app_url)) = (key, app_url) else {
            unreachable!("a manifest without problems has a key and an app URL")
        };
        Ok(Self {
            document,
            key,
            app_url,
            redirect_uris,
            scopes,
            webhook,
            custom_fields,
        })
    }
}

#[derive(Default)]
pub(crate) struct Checker {
    pub(crate) problems: Vec<ManifestProblem>,
}

impl Checker {
    pub(crate) fn add(&mut self, path: &str, message: impl Into<String>) {
        self.problems.push(ManifestProblem {
            path: path.to_string(),
            message: message.into(),
        });
    }

    pub(crate) fn text(&mut self, path: &str, value: &str, max: usize) {
        if value.trim().is_empty() || value.chars().count() > max {
            self.add(path, format!("is 1 to {max} characters"));
        }
    }

    fn count(&mut self, path: &str, n: usize, max: usize) {
        if n > max {
            self.add(path, format!("lists at most {max}"));
        }
    }

    fn id(&mut self, path: &str, id: &str, seen: &mut Vec<String>) {
        let valid = !id.is_empty()
            && id.len() <= 64
            && id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_'));
        if !valid {
            self.add(
                &format!("{path}.id"),
                "is 1 to 64 lowercase letters, digits, `-` or `_`",
            );
        } else if seen.iter().any(|s| s == id) {
            self.add(&format!("{path}.id"), format!("{id:?} is used twice"));
        }
        seen.push(id.to_string());
    }

    fn target(&mut self, path: &str, target: &str) {
        if !is_point_name(target) && !is_word(target) {
            self.add(
                &format!("{path}.target"),
                "is a target name (`order.payment`, `payment`)",
            );
        }
    }
}

fn is_word(w: &str) -> bool {
    !w.is_empty() && w.len() <= 64 && w.chars().all(|c| c.is_ascii_lowercase() || c == '_')
}

/// `payments.start`: two or more lowercase words joined by dots.
fn is_point_name(name: &str) -> bool {
    let parts: Vec<&str> = name.split('.').collect();
    parts.len() >= 2 && parts.iter().all(|p| is_word(p))
}

/// A valid custom element name, the common subset: `[a-z][a-z0-9]*(-[a-z0-9]+)+`.
fn is_element_tag(tag: &str) -> bool {
    let mut parts = tag.split('-');
    let first = parts.next().unwrap_or("");
    let rest: Vec<&str> = parts.collect();
    let word = |w: &str| {
        !w.is_empty()
            && w.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    };
    first.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && word(first)
        && !rest.is_empty()
        && rest.iter().all(|w| word(w))
        && tag.len() <= 64
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    pub(crate) fn stripe() -> serde_json::Value {
        json!({
            "$schema": SCHEMA_URL,
            "key": "app_stripe",
            "name": "Stripe",
            "version": "1.2.0",
            "publisher": "Hypha",
            "app_url": "https://stripe-app.example.com",
            "redirect_uris": ["/hypha/oauth/callback"],
            "scopes": ["orders:read", "customer_data:email", "payments:write"],
            "scope_reasons": { "customer_data:email": "Stripe sends the receipt to the buyer" },
            "privacy_url": "/privacy",
            "webhooks": { "url": "/hypha/webhooks", "events": ["order.cancelled"] },
            "extension_points": [
                { "point": "payments.start", "version": 1, "url": "/hypha/payments/start" }
            ],
            "settings": { "schema": { "type": "object" }, "secret_fields": ["secret_key"] },
            "admin": {
                "pages": [{ "id": "dashboard", "title": "Stripe", "url": "/admin", "nav": "apps" }],
                "blocks": [{ "id": "payment", "target": "order.payment", "url": "/hypha/blocks/payment" }],
                "actions": [{ "id": "capture", "target": "payment", "label": "Capture",
                              "point": "payments.capture" }],
                "proxy": "/hypha/admin-api"
            },
            "storefront": {
                "elements": [{ "id": "payment-element", "target": "checkout.payment",
                               "module": "/elements/payment.js", "tag": "stripe-payment" }],
                "proxy": "/hypha/storefront"
            }
        })
    }

    fn parse(value: serde_json::Value) -> Result<AppManifest, ManifestError> {
        AppManifest::parse(&serde_json::to_vec(&value).unwrap())
    }

    fn problems(value: serde_json::Value) -> Vec<(String, String)> {
        parse(value)
            .unwrap_err()
            .problems
            .into_iter()
            .map(|p| (p.path, p.message))
            .collect()
    }

    #[test]
    fn a_manifest_cant_name_a_spore() {
        let mut manifest = stripe();
        manifest["key"] = json!("spore_stripe");
        assert_eq!(
            problems(manifest),
            vec![(
                "key".to_string(),
                "spore_stripe is a spore's key: spores are built into the API, apps are \"app_<name>\""
                    .to_string()
            )]
        );
    }

    #[test]
    fn a_full_manifest_parses_with_relative_urls_resolved() {
        let manifest = parse(stripe()).unwrap();
        assert_eq!(manifest.key().as_str(), "app_stripe");
        assert_eq!(
            manifest.scopes(),
            &[
                Scope::OrdersRead,
                Scope::PaymentsWrite,
                Scope::CustomerDataEmail
            ]
        );
        assert!(manifest.allows_redirect("https://stripe-app.example.com/hypha/oauth/callback"));
        assert!(!manifest.allows_redirect("https://stripe-app.example.com/hypha/oauth/callback/"));
        assert_eq!(
            manifest.privacy_url(),
            "https://stripe-app.example.com/privacy"
        );
        let (url, events) = manifest.webhook().unwrap();
        assert_eq!(
            url.as_str(),
            "https://stripe-app.example.com/hypha/webhooks"
        );
        assert_eq!(events[0].as_str(), "order.cancelled");
        let doc = manifest.document();
        assert_eq!(
            doc.extension_points[0].url,
            "https://stripe-app.example.com/hypha/payments/start"
        );
        assert_eq!(
            doc.storefront.as_ref().unwrap().elements[0].module,
            "https://stripe-app.example.com/elements/payment.js"
        );
        assert_eq!(
            doc.admin.as_ref().unwrap().proxy.as_deref(),
            Some("https://stripe-app.example.com/hypha/admin-api")
        );
        assert_eq!(
            manifest.reason(Scope::CustomerDataEmail),
            Some("Stripe sends the receipt to the buyer")
        );
    }

    #[test]
    fn a_stored_copy_round_trips() {
        let manifest = parse(stripe()).unwrap();
        assert_eq!(AppManifest::from_value(manifest.to_value()), Ok(manifest));
    }

    #[test]
    fn personal_data_scopes_need_a_reason() {
        let mut m = stripe();
        m["scope_reasons"] = json!({});
        assert_eq!(
            problems(m),
            vec![(
                "scope_reasons".to_string(),
                "says why the app needs customer_data:email: personal data needs a reason"
                    .to_string()
            )]
        );
    }

    #[test]
    fn every_problem_is_reported_with_its_path() {
        let mut m = stripe();
        m["key"] = json!("stripe");
        m["scopes"] = json!(["orders:read", "orders:read", "orders:delete"]);
        m["scope_reasons"] = json!({ "products:read": "x" });
        m["redirect_uris"] = json!(["ftp://example.com/cb", "/cb#x"]);
        m["admin"]["pages"][0]["id"] = json!("Dash Board");
        m["storefront"]["elements"][0]["tag"] = json!("stripe");
        let found: Vec<String> = problems(m).into_iter().map(|(p, _)| p).collect();
        assert_eq!(
            found,
            vec![
                "key",
                "redirect_uris[0]",
                "redirect_uris[1]",
                "scopes[1]",
                "scopes[2]",
                "scope_reasons.products:read",
                "admin.pages[0].id",
                "storefront.elements[0].tag",
            ]
        );
    }

    #[test]
    fn http_is_only_for_localhost_and_a_privacy_policy_is_required() {
        let mut m = stripe();
        m["app_url"] = json!("http://stripe-app.example.com");
        assert_eq!(problems(m.clone())[0].0, "app_url");
        m["app_url"] = json!("http://localhost:4000");
        let manifest = parse(m.clone()).unwrap();
        assert!(manifest.allows_redirect("http://localhost:4000/hypha/oauth/callback"));
        m.as_object_mut().unwrap().remove("privacy_url");
        assert_eq!(problems(m)[0].0, "");
    }

    #[test]
    fn unknown_fields_and_huge_documents_are_refused() {
        let mut m = stripe();
        m["colour"] = json!("blue");
        let found = problems(m);
        assert_eq!(found[0].0, "");
        assert!(found[0].1.contains("unknown field `colour`"), "{found:?}");
        let huge = vec![b' '; MAX_MANIFEST_BYTES + 1];
        assert_eq!(
            AppManifest::parse(&huge).unwrap_err().problems[0].message,
            format!("is larger than {MAX_MANIFEST_BYTES} bytes")
        );
    }

    #[test]
    fn an_action_names_a_point_or_a_url() {
        let mut m = stripe();
        m["admin"]["actions"][0]["url"] = json!("/capture");
        assert_eq!(
            problems(m),
            vec![(
                "admin.actions[0]".to_string(),
                "names either a `point` or a `url`".to_string()
            )]
        );
    }

    #[test]
    fn element_tags_follow_custom_element_names() {
        assert!(is_element_tag("stripe-payment"));
        assert!(is_element_tag("x-1"));
        for bad in ["stripe", "Stripe-pay", "1x-pay", "x-", "-x", "x_y-z"] {
            assert!(!is_element_tag(bad), "{bad}");
        }
    }

    #[test]
    fn a_served_manifest_needs_a_redirect_uri_and_an_uploaded_one_doesnt() {
        let mut m = stripe();
        m["redirect_uris"] = json!([]);
        assert_eq!(
            problems(m.clone()),
            vec![(
                "redirect_uris".to_string(),
                "lists 1 to 10 URLs".to_string()
            )]
        );
        let bytes = serde_json::to_vec(&m).unwrap();
        let uploaded = AppManifest::parse_from(&bytes, ManifestSource::Uploaded).unwrap();
        assert!(uploaded.redirect_uris().is_empty());
        assert_eq!(
            AppManifest::from_value_of(uploaded.to_value(), ManifestSource::Uploaded),
            Ok(uploaded.clone())
        );
        assert!(AppManifest::from_value(uploaded.to_value()).is_err());
    }

    #[test]
    fn private_client_ids_are_urns_of_the_key() {
        let key = AppKey::new("app_rates").unwrap();
        let id = private_client_id(&key);
        assert_eq!(id, "urn:hypha:private:app_rates");
        assert!(is_private_client(&id));
        assert!(!is_private_client("https://rates.example/hypha-app.json"));
        assert_eq!(ManifestSource::of_client(&id), ManifestSource::Uploaded);
    }

    /// The published schema (`docs/apps/manifest.schema.json`) is generated from
    /// `ManifestDocument`; `HYPHA_WRITE_MANIFEST_SCHEMA=1 cargo test -p hypha-apps
    /// manifest_schema` rewrites it after the types change.
    #[test]
    fn the_published_manifest_schema_is_current() {
        let mut schema = serde_json::to_value(schemars::schema_for!(ManifestDocument)).unwrap();
        schema["$id"] = json!(SCHEMA_URL);
        let text = format!("{}\n", serde_json::to_string_pretty(&schema).unwrap());
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/apps/manifest.schema.json"
        );
        if std::env::var_os("HYPHA_WRITE_MANIFEST_SCHEMA").is_some() {
            std::fs::write(path, &text).unwrap();
        }
        let published = std::fs::read_to_string(path).unwrap_or_default();
        assert!(
            published == text,
            "docs/apps/manifest.schema.json is stale: run \
             `HYPHA_WRITE_MANIFEST_SCHEMA=1 cargo test -p hypha-apps manifest_schema`"
        );
    }
}
