//! `hypha.config.toml` as written (design note 0017, section 3): plain data, every key
//! optional, unknown keys refused. The doc comments are the schema's descriptions, which
//! editors show on hover. Defaults come from the domain crates' settings, so each value
//! has one source.
//!
//! Key names are a contract with operators (CF6): never rename one once released.

use std::collections::BTreeMap;
use std::time::Duration;

use hypha_auth::AuthSettings;
use hypha_categories::CategorySettings;
use hypha_dispatch::DispatchSettings;
use hypha_extensions::ExtensionSettings;
use hypha_media::MediaSettings;
use hypha_notifications::SendSettings;
use hypha_privacy::PrivacySettings;
use hypha_search::SyncSettings;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::units::{Dur, Size};

/// Hypha's settings for one deployment. Secrets (`DATABASE_URL`, `HYPHA_SECRET_KEY`, the
/// S3 keys, the email provider's password or key) stay in environment variables and
/// have no key here.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
#[schemars(title = "hypha.config.toml")]
pub struct File {
    pub server: Server,
    pub database: Database,
    pub outbound: Outbound,
    pub media: Media,
    pub auth: Auth,
    pub email: Email,
    pub categories: Categories,
    pub customers: Customers,
    pub webhooks: Webhooks,
    pub jobs: Jobs,
    pub rate_limits: RateLimits,
    /// Settings for the spores this API is built with (design note 0030, §8): one table
    /// per spore, `[spores.<name>]`, read by the spore itself; its keys are the spore's
    /// to document. A table for a spore the build doesn't have stops startup.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    #[schemars(with = "BTreeMap<String, serde_json::Map<String, serde_json::Value>>")]
    pub spores: BTreeMap<String, toml::Table>,
}

/// The HTTP server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Server {
    /// The address to listen on, `host:port`. `HYPHA_BIND` wins, then `PORT` (as
    /// `0.0.0.0:$PORT`, as platforms like Fly.io and Render expect).
    pub bind: String,
    /// The proxies and load balancers whose `X-Forwarded-For` is believed: CIDR ranges
    /// or addresses. Behind one, set it, or every client shares one rate limit.
    /// `HYPHA_TRUSTED_PROXIES` (comma-separated) wins.
    pub trusted_proxies: Vec<String>,
    /// Where the admin panel lives, for links in emails; unset, emails show the bare
    /// token. `HYPHA_ADMIN_URL` wins.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub admin_url: Option<String>,
    /// The API's public base URL (`https://api.shop.example`), as apps reach it: the
    /// OAuth issuer and endpoints in its metadata, the `issuer` of token answers, the
    /// `iss` of app session tokens, a private app's `token_endpoint`. Unset, it's
    /// derived from each request: `X-Forwarded-Host` and `-Proto` from a trusted proxy
    /// (`trusted_proxies`), else the `Host` header. `HYPHA_PUBLIC_URL` wins.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_url: Option<String>,
    /// The largest request body accepted: 1KiB to 64MiB. Files don't count: they're
    /// uploaded to storage directly. Below 256KiB, a product's longest description may
    /// not fit.
    pub max_body: Size,
    /// How long stopping waits for jobs, and then for background work, to finish. At
    /// least 1 second.
    pub shutdown_grace: Dur,
}

impl Default for Server {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:3000".to_string(),
            trusted_proxies: Vec::new(),
            admin_url: None,
            public_url: None,
            max_body: Size(2 << 20),
            shutdown_grace: Dur(Duration::from_secs(10)),
        }
    }
}

/// The Postgres connection pool. The URL is `DATABASE_URL`, never here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Database {
    /// How long a request waits for a pooled connection (or for a new one to open, or an
    /// old one to be checked) before it fails.
    pub pool_timeout: Dur,
    /// The most connections open at once. Unset: the pool's default, 4 per CPU.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_connections: Option<u32>,
    /// Prepare each query once per connection and reuse it, instead of having Postgres
    /// plan it on every call (most of its work for simple reads). Turn it off behind a
    /// pooler in transaction mode that doesn't keep prepared statements (PgBouncer
    /// before 1.21, or without `max_prepared_statements`).
    pub statement_cache: bool,
}

impl Default for Database {
    fn default() -> Self {
        Self {
            pool_timeout: Dur(Duration::from_secs(5)),
            max_connections: None,
            statement_cache: true,
        }
    }
}

/// Calls to URLs merchants configure (rate apps, webhooks).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Outbound {
    /// Lets those calls reach private and loopback addresses, for apps on localhost.
    /// Development only. `HYPHA_ALLOW_PRIVATE_CALLBACKS` wins.
    pub allow_private: bool,
}

/// File storage and asset URLs. Media is off without `s3.endpoint`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Media {
    /// Where public files are served from, e.g. a CDN. Unset: every file gets a signed
    /// URL. `HYPHA_MEDIA_URL` wins.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_url: Option<String>,
    /// How thumbnails are made from public image URLs. `HYPHA_MEDIA_TRANSFORMS` wins.
    pub transforms: Transforms,
    /// How long an upload URL works (1 minute to 7 days).
    pub upload_expires: Dur,
    /// How long a private file's download URL works (1 minute to 7 days).
    pub download_expires: Dur,
    /// How long an unfinished upload is kept before the sweep deletes it. At least
    /// `upload_expires`.
    pub abandoned_after: Dur,
    pub s3: S3,
}

impl Default for Media {
    fn default() -> Self {
        let d = MediaSettings::default();
        Self {
            public_url: None,
            transforms: Transforms::None,
            upload_expires: Dur(d.upload_expires),
            download_expires: Dur(d.download_expires),
            abandoned_after: Dur(d.abandoned_after),
            s3: S3::default(),
        }
    }
}

/// How thumbnails are made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Transforms {
    /// No resizing: the thumbnail is the original.
    #[default]
    None,
    /// Cloudflare Image Transformations on the media domain's zone.
    Cloudflare,
}

/// An S3-compatible bucket (R2, S3, MinIO). The keys are `HYPHA_S3_ACCESS_KEY_ID` and
/// `HYPHA_S3_SECRET_ACCESS_KEY`, never here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct S3 {
    /// The S3 API's URL. Unset: media is off. `HYPHA_S3_ENDPOINT` wins.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    /// `HYPHA_S3_REGION` wins.
    pub region: String,
    /// Required with an endpoint. `HYPHA_S3_BUCKET` wins.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bucket: Option<String>,
    /// Creates the bucket on startup if it's missing. Development only.
    /// `HYPHA_S3_CREATE_BUCKET` wins.
    pub create_bucket: bool,
}

impl Default for S3 {
    fn default() -> Self {
        Self {
            endpoint: None,
            region: "auto".to_string(),
            bucket: None,
            create_bucket: false,
        }
    }
}

/// How long staff sessions and auth's emailed tokens last. Each is at most 365 days.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Auth {
    /// How long a session lasts from login. At least 1 minute.
    pub session_lifetime: Dur,
    /// How long, after the password, the second factor may be entered.
    pub second_factor_window: Dur,
    /// How long second-factor checks stop after 5 wrong codes in a row.
    pub second_factor_lock: Dur,
    /// How long an emailed password reset link works.
    pub password_reset_lifetime: Dur,
    /// While the latest reset link is younger than this, unused and still working,
    /// asking again sends nothing.
    pub password_reset_resend_after: Dur,
    /// How long an invite works. At least 1 minute.
    pub invite_lifetime: Dur,
}

impl Default for Auth {
    fn default() -> Self {
        let d = AuthSettings::default();
        Self {
            session_lifetime: Dur(d.session_lifetime),
            second_factor_window: Dur(d.second_factor_window),
            second_factor_lock: Dur(d.second_factor_lock),
            password_reset_lifetime: Dur(d.password_reset_lifetime),
            password_reset_resend_after: Dur(d.password_reset_resend_after),
            invite_lifetime: Dur(d.invite_lifetime),
        }
    }
}

/// How emails go out (design note 0028): order confirmations, shipping updates, staff
/// invites and password resets.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Email {
    /// Who sends them: `log` writes each one to the log instead (development only),
    /// `smtp` uses `email.smtp` (not on Workers), `resend` Resend's API with the key in
    /// `HYPHA_RESEND_API_KEY`. `HYPHA_EMAIL_SENDER` wins.
    pub sender: EmailSender,
    /// The address emails come from, on a domain the provider verified; the shop's name
    /// (notification settings) is shown with it. Required unless `sender` is `log`.
    /// `HYPHA_EMAIL_FROM` wins.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    pub smtp: Smtp,
}

/// Who sends emails.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum EmailSender {
    /// Written to the log, not sent.
    #[default]
    Log,
    /// An SMTP server (`email.smtp`): any provider's, or Mailpit locally.
    Smtp,
    /// Resend's API; the key is `HYPHA_RESEND_API_KEY`.
    Resend,
}

impl EmailSender {
    /// As the file writes it.
    pub fn as_str(self) -> &'static str {
        match self {
            EmailSender::Log => "log",
            EmailSender::Smtp => "smtp",
            EmailSender::Resend => "resend",
        }
    }
}

/// The SMTP server, when `email.sender` is `smtp`. The password is `HYPHA_SMTP_PASSWORD`,
/// never here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Smtp {
    /// The server's host name. `HYPHA_SMTP_HOST` wins.
    pub host: String,
    /// Its port: usually 587 with `starttls`, 465 with `tls`, 25 or 1025 (Mailpit) with
    /// `none`. `HYPHA_SMTP_PORT` wins.
    pub port: u16,
    /// How the connection is encrypted. `HYPHA_SMTP_TLS` wins.
    pub tls: SmtpTls,
    /// The login, if the server wants one; its password is `HYPHA_SMTP_PASSWORD`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
}

impl Default for Smtp {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 587,
            tls: SmtpTls::Starttls,
            username: None,
        }
    }
}

/// How an SMTP connection is encrypted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SmtpTls {
    /// Plain text: a server on this machine or network (Mailpit) only.
    None,
    /// Plain, then upgraded with STARTTLS, which is required (port 587).
    #[default]
    Starttls,
    /// TLS from the first byte (port 465).
    Tls,
}

/// The product category tree (design note 0022).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Categories {
    /// How many levels categories nest, 1 to 10. Lowering it leaves deeper categories
    /// where they are: it applies to creating and moving.
    pub max_depth: u8,
}

impl Default for Categories {
    fn default() -> Self {
        Self {
            max_depth: CategorySettings::default().max_depth,
        }
    }
}

/// Customers and their personal data (design note 0032).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Customers {
    /// How long a requested erasure of a customer's personal data can be cancelled
    /// before the job runs it. At least 1 hour; 10 days by default. A merchant under a
    /// retention duty sets it longer.
    pub erasure_delay: Dur,
}

impl Default for Customers {
    fn default() -> Self {
        Self {
            erasure_delay: Dur(PrivacySettings::default().erasure_delay),
        }
    }
}

/// When failed deliveries (webhooks and in-process handlers) are tried again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Webhooks {
    /// The waits after the first failures, in order (at most 20, each at most 365 days).
    pub retry: Vec<Dur>,
    /// The wait after each later failure. At most 365 days.
    pub retry_then_every: Dur,
    /// No attempt is planned more than this long after the first; then the delivery
    /// fails for good (and can be retried from the admin). At least the first wait, at
    /// most 365 days.
    pub give_up_after: Dur,
}

impl Default for Webhooks {
    fn default() -> Self {
        let d = DispatchSettings::default().retry;
        Self {
            retry: d.backoff.into_iter().map(Dur).collect(),
            retry_then_every: Dur(d.then_every),
            give_up_after: Dur(d.give_up_after),
        }
    }
}

/// Background jobs. Set `enabled = false` on instances that only serve requests, and
/// keep them on in one. A Workers host runs them from Cron triggers and ignores the
/// intervals.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Jobs {
    /// `false`: this instance runs no jobs, whatever the jobs below say.
    pub enabled: bool,
    /// The longest a job with nothing to do waits before looking again. An idle job
    /// doubles its wait from its `interval` up to this, and drops back as soon as it
    /// finds work; writes wake the dispatcher (and it search) at once, so changes don't
    /// wait for it. Long enough, a database that bills while busy can rest.
    pub idle_interval: Dur,
    pub dispatch: DispatchJob,
    pub prune: PruneJob,
    pub media_sweep: Job,
    pub search_sync: SearchSyncJob,
    pub send_notifications: SendNotificationsJob,
}

impl Default for Jobs {
    fn default() -> Self {
        let hour = Dur(Duration::from_secs(3600));
        Self {
            enabled: true,
            idle_interval: Dur(Duration::from_secs(60)),
            dispatch: DispatchJob::default(),
            prune: PruneJob::default(),
            search_sync: SearchSyncJob::default(),
            send_notifications: SendNotificationsJob::default(),
            media_sweep: Job {
                enabled: true,
                interval: hour,
            },
        }
    }
}

/// A job that runs every `interval`, and again at once while it has more to do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Job {
    pub enabled: bool,
    pub interval: Dur,
}

impl Default for Job {
    fn default() -> Self {
        Self {
            enabled: true,
            interval: Dur(Duration::from_secs(3600)),
        }
    }
}

/// Delivering events: fanning them out to apps and handlers, and running what's due.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct DispatchJob {
    pub enabled: bool,
    pub interval: Dur,
    /// Events fanned out, deliveries run, or old rows pruned per step (1 to 1,000).
    pub batch: u32,
    /// How long a runner holds the deliveries it took; past it, another may take them.
    /// At least 1 minute: the slowest webhook may take 45 seconds.
    pub lease: Dur,
}

impl Default for DispatchJob {
    fn default() -> Self {
        let d = DispatchSettings::default();
        Self {
            enabled: true,
            interval: Dur(Duration::from_secs(2)),
            batch: d.batch,
            lease: Dur(d.lease),
        }
    }
}

/// Keeping search up to date (design note 0019): rebuilding the products events marked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct SearchSyncJob {
    pub enabled: bool,
    pub interval: Dur,
    /// Products rebuilt per step, each in every market (1 to 1,000).
    pub batch: u32,
    /// How long a runner holds the products it took; past it, another may take them.
    /// At least 10 seconds.
    pub lease: Dur,
}

impl Default for SearchSyncJob {
    fn default() -> Self {
        let s = SyncSettings::default();
        Self {
            enabled: true,
            interval: Dur(Duration::from_secs(2)),
            batch: s.batch,
            lease: Dur(s.lease),
        }
    }
}

/// Sending the emails the store queued (design note 0028, §4): order emails and
/// resends. Staff invites and password resets are sent by their request, not by this
/// job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct SendNotificationsJob {
    pub enabled: bool,
    pub interval: Dur,
    /// Emails sent per step (1 to 1,000).
    pub batch: u32,
    /// How long a runner holds the emails it took; past it, another may take them (and
    /// a provider without idempotency keys, SMTP, may then get one twice). At least 20
    /// seconds: a send may take 15.
    pub lease: Dur,
    /// The waits after each failure the provider says may pass (a timeout, a 5xx, a
    /// rate limit), in order; after the last, the email fails for good and staff can
    /// resend it. At most 20, each 1 second to 365 days.
    pub retry: Vec<Dur>,
}

impl Default for SendNotificationsJob {
    fn default() -> Self {
        let s = SendSettings::default();
        Self {
            enabled: true,
            interval: Dur(Duration::from_secs(2)),
            batch: s.batch,
            lease: Dur(s.lease),
            retry: s.retry.into_iter().map(Dur).collect(),
        }
    }
}

/// Deleting what's past keeping.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct PruneJob {
    pub enabled: bool,
    pub interval: Dur,
    /// How long events, and their deliveries, are kept. At least a day longer than
    /// `webhooks.give_up_after`, so no delivery is deleted while it's still due.
    pub events_after: Dur,
    /// How long an `Idempotency-Key` is honoured. At least 1 hour.
    pub idempotency_keys_after: Dur,
    /// How long a session is kept after it ended (expired or revoked).
    pub sessions_after: Dur,
    /// How long the log of calls to apps' extension points is kept (design note 0029).
    pub extension_calls_after: Dur,
}

impl Default for PruneJob {
    fn default() -> Self {
        let d = DispatchSettings::default();
        Self {
            enabled: true,
            interval: Dur(Duration::from_secs(3600)),
            events_after: Dur(d.keep_events),
            idempotency_keys_after: Dur(d.keep_idempotency_keys),
            sessions_after: Dur(d.keep_ended_sessions),
            extension_calls_after: Dur(ExtensionSettings::default().keep_calls),
        }
    }
}

/// Up to `burst` requests at once, then one more each `per`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Quota {
    /// At least 1.
    pub burst: u32,
    pub per: Dur,
}

const fn quota(burst: u32, secs: u64) -> Quota {
    Quota {
        burst,
        per: Dur(Duration::from_secs(secs)),
    }
}

/// Limits on the routes that need no credentials, and on the storefront routes that
/// call rate apps (design notes 0009 and 0015). Counted per instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct RateLimits {
    /// Logging in, per client address.
    pub login_ip: Quota,
    /// Logging in, per email and address: guesses at one account from one place.
    pub login_email_from_ip: Quota,
    /// Logging in, per email from anywhere: guessing spread over many addresses.
    pub login_email: Quota,
    /// The second factor, per client address.
    pub mfa_ip: Quota,
    /// Asking for a password reset, per client address.
    pub reset_ip: Quota,
    /// Asking for a password reset, per email and address.
    pub reset_email_from_ip: Quota,
    /// Setting a new password with a reset token, per client address.
    pub reset_confirm_ip: Quota,
    /// Accepting an invite, per client address.
    pub invite_ip: Quota,
    /// Shipping quotes, per cart: each may call rate apps.
    pub shipping_quotes_cart: Quota,
    /// Creating carts, per client address.
    pub cart_create_ip: Quota,
    /// The OAuth endpoints (authorize, token, revoke), per client address.
    pub oauth_ip: Quota,
    /// Spores' public routes (`/spores/<name>/…`: a payment provider's webhooks), per
    /// client address, all spores together.
    pub spore_public: Quota,
}

/// The starting numbers from design note 0009, section 3.
impl Default for RateLimits {
    fn default() -> Self {
        Self {
            login_ip: quota(10, 30),
            login_email_from_ip: quota(5, 60),
            // About 50 an hour.
            login_email: quota(50, 72),
            mfa_ip: quota(10, 30),
            reset_ip: quota(5, 60),
            reset_email_from_ip: quota(3, 5 * 60),
            reset_confirm_ip: quota(10, 30),
            invite_ip: quota(10, 30),
            // A checkout asks a few times; a page polling every second doesn't need to.
            shipping_quotes_cart: quota(10, 2),
            // A shopper makes a cart or two; a page reloading makes a few more.
            cart_create_ip: quota(20, 3),
            // An app installs or refreshes now and then; a stuck client loops.
            oauth_ip: quota(20, 3),
            // A provider sends webhooks in bursts, from a few addresses.
            spore_public: Quota {
                burst: 120,
                per: Dur(Duration::from_millis(250)),
            },
        }
    }
}

impl RateLimits {
    /// Every limit with its key, for the checks.
    pub(crate) fn all(&self) -> [(&'static str, Quota); 12] {
        [
            ("login_ip", self.login_ip),
            ("login_email_from_ip", self.login_email_from_ip),
            ("login_email", self.login_email),
            ("mfa_ip", self.mfa_ip),
            ("reset_ip", self.reset_ip),
            ("reset_email_from_ip", self.reset_email_from_ip),
            ("reset_confirm_ip", self.reset_confirm_ip),
            ("invite_ip", self.invite_ip),
            ("shipping_quotes_cart", self.shipping_quotes_cart),
            ("cart_create_ip", self.cart_create_ip),
            ("oauth_ip", self.oauth_ip),
            ("spore_public", self.spore_public),
        ]
    }
}
