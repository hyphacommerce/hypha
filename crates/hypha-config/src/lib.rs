//! Hypha's configuration: `hypha.config.toml`, the environment variables that win over it,
//! the checks, and the settings each crate takes.
//!
//! It parses a string and reads the environment through a function it's given, so it builds
//! for wasm: the native host passes the file's text and `std::env::var`, a Workers host its
//! bindings, tests a map. Only hosts depend on it.
//!
//! ```text
//! defaults ─► hypha.config.toml ─► environment ─► checks ─► Config
//! ```

pub mod file;
pub mod units;

use core::time::Duration;
use std::collections::BTreeMap;

use hypha_auth::AuthSettings;
use hypha_categories::CategorySettings;
use hypha_core::ErrorCode;
use hypha_dispatch::{DispatchSettings, LONGEST_RUN, RetrySchedule};
use hypha_extensions::ExtensionSettings;
use hypha_media::MediaSettings;
use hypha_notifications::{LONGEST_SEND, SendSettings};
use hypha_search::SyncSettings;
use ipnet::IpNet;
use thiserror::Error;

pub use file::{AgentWrites, EmailSender, File, Quota, RateLimits, SmtpTls, Transforms};
pub use units::{Dur, Size, UnitError};

/// The file read when `HYPHA_CONFIG` names none, if it's there.
pub const DEFAULT_PATH: &str = "hypha.config.toml";

/// The settings in effect: checked, and in the types each crate takes.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    /// Whether calls to merchant-configured URLs may reach private addresses.
    pub allow_private_callbacks: bool,
    pub media: MediaConfig,
    pub auth: AuthSettings,
    /// Staff accounts locked against changing themselves (`auth.locked_accounts`).
    pub staff: hypha_users::StaffSettings,
    pub email: EmailConfig,
    pub categories: CategorySettings,
    /// The erasure window.
    pub privacy: hypha_privacy::PrivacySettings,
    /// Customer sessions and sign-in codes.
    pub accounts: hypha_customer_accounts::AccountSettings,
    pub dispatch: DispatchSettings,
    /// The search sync job's batch and lease.
    pub search_sync: SyncSettings,
    /// The send job's batch, lease and retries.
    pub send_notifications: SendSettings,
    /// How long the extension call log is kept.
    pub extensions: ExtensionSettings,
    pub jobs: JobsConfig,
    pub rate_limits: RateLimits,
    pub agents: AgentsConfig,
    /// Each spore's section, `[spores.<name>]`, as written: the spore reads its own when
    /// the host builds it, and the host refuses a section for a spore it wasn't built with.
    pub spores: BTreeMap<String, toml::Table>,
    /// The file with the environment applied, for `Config::to_toml`.
    effective: File,
}

/// Agents, and previews of writes (`[agents]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentsConfig {
    /// Whether agents may act at all.
    pub enabled: bool,
    /// The operator's cap on what any agent writes.
    pub writes: hypha_agents::WritesCap,
    /// The statement timeout of a previewed write's transaction.
    pub preview_timeout: Duration,
    /// How long a preview's token applies its write.
    pub preview_token_lifetime: Duration,
    /// How long proposals wait and the call log is kept.
    pub settings: hypha_agents::AgentSettings,
}

impl Default for AgentsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            writes: hypha_agents::WritesCap::Apply,
            preview_timeout: Duration::from_secs(5),
            // Long enough to read a preview and send it back; short enough that what it
            // previewed is still what's there.
            preview_token_lifetime: Duration::from_secs(15 * 60),
            settings: hypha_agents::AgentSettings::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerConfig {
    /// `host:port`.
    pub bind: String,
    pub trusted_proxies: Vec<IpNet>,
    /// Without a trailing `/`.
    pub admin_url: Option<String>,
    /// The API's public base URL, without a trailing `/` (`server.public_url`).
    pub public_url: Option<String>,
    pub max_body: usize,
    pub shutdown_grace: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseConfig {
    pub pool_timeout: Duration,
    pub max_connections: Option<usize>,
    /// Whether pooled connections keep their prepared statements.
    pub statement_cache: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaConfig {
    /// `None`: media is off.
    pub s3: Option<S3Config>,
    pub public_url: Option<String>,
    pub transforms: Transforms,
    pub settings: MediaSettings,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct S3Config {
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub create_bucket: bool,
}

/// How emails go out. The secrets (`HYPHA_SMTP_PASSWORD`, `HYPHA_RESEND_API_KEY`) aren't
/// here: the hosts read them when they build the sender.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailConfig {
    pub sender: SenderConfig,
    /// The address emails come from: `email.from`, or `EmailConfig::LOG_FROM` for the
    /// log without one.
    pub from: hypha_core::Email,
}

impl EmailConfig {
    /// Where logged emails say they're from when `email.from` is unset.
    pub const LOG_FROM: &str = "shop@example.com";
}

/// Who sends emails, with what it needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SenderConfig {
    Log,
    Smtp(SmtpConfig),
    Resend,
}

impl SenderConfig {
    /// As `email.sender` names it.
    pub fn name(&self) -> &'static str {
        match self {
            SenderConfig::Log => EmailSender::Log.as_str(),
            SenderConfig::Smtp(_) => EmailSender::Smtp.as_str(),
            SenderConfig::Resend => EmailSender::Resend.as_str(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub tls: SmtpTls,
    pub username: Option<String>,
}

/// Each job's switch (already off when `jobs.enabled` is) and interval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobsConfig {
    /// The longest an idle job waits (`jobs.idle_interval`).
    pub idle_interval: Duration,
    pub dispatch: JobConfig,
    pub prune: JobConfig,
    pub media_sweep: JobConfig,
    pub search_sync: JobConfig,
    pub send_notifications: JobConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobConfig {
    pub enabled: bool,
    pub interval: Duration,
}

/// Why the configuration can't be used.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ConfigError {
    /// The file isn't TOML, or has a key or value of the wrong kind; the message names
    /// the line and column.
    #[error("{0}")]
    Parse(#[from] toml::de::Error),
    /// Values that parse but break a rule, each with its key.
    #[error("{}", list(.0))]
    Invalid(Vec<Problem>),
}

impl ErrorCode for ConfigError {
    fn code(&self) -> &'static str {
        match self {
            ConfigError::Parse(_) => "config_parse",
            ConfigError::Invalid(_) => "config_invalid",
        }
    }
}

/// One broken rule: the key (`jobs.dispatch.lease`, or the environment variable that
/// set it) and what's wrong.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{key}: {broken}")]
pub struct Problem {
    pub key: String,
    pub broken: Broken,
}

/// What's wrong with a value.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum Broken {
    /// Below its minimum; `because` names the setting the minimum comes from, if any.
    #[error("is {value}, at least {min} is needed{}", because_of(*.because))]
    TooShort {
        value: Dur,
        min: Dur,
        because: Option<&'static str>,
    },
    #[error("is {value}, at most {max}")]
    TooLong { value: Dur, max: Dur },
    #[error("is {value}, {min} to {max}")]
    SizeOutOfRange { value: Size, min: Size, max: Size },
    #[error("is {value}, {min} to {max}")]
    NumberOutOfRange { value: u64, min: u64, max: u64 },
    #[error("has {count} waits, at most {max}")]
    TooManyWaits { count: usize, max: usize },
    #[error("is at least 1")]
    Zero,
    #[error("{0:?} isn't host:port, like \"0.0.0.0:3000\"")]
    NotHostPort(String),
    #[error("not CIDR ranges or addresses: {0:?}")]
    NotNetworks(Vec<String>),
    #[error("{0:?} isn't an http:// or https:// URL")]
    NotUrl(String),
    #[error("is true or false, not {0:?}")]
    NotFlag(String),
    #[error("is `cloudflare` or `none`, not {0:?}")]
    NotTransforms(String),
    #[error("is required with an endpoint (or HYPHA_S3_BUCKET)")]
    BucketRequired,
    #[error("{0:?} isn't an email address")]
    NotEmail(String),
    #[error("is required with sender `{0}`")]
    FromRequired(&'static str),
    #[error("is `log`, `smtp` or `resend`, not {0:?}")]
    NotSender(String),
    #[error("is `none`, `starttls` or `tls`, not {0:?}")]
    NotTls(String),
    #[error("{0:?} isn't a port, 1 to 65535")]
    NotPort(String),
    #[error("can't be empty")]
    Empty,
    #[error("is `apply`, `propose` or `off`, not {0:?}")]
    NotAgentWrites(String),
}

fn because_of(key: Option<&str>) -> String {
    key.map(|key| format!(" ({key})")).unwrap_or_default()
}

impl ErrorCode for Broken {
    fn code(&self) -> &'static str {
        match self {
            Broken::TooShort { .. } => "config_too_short",
            Broken::TooLong { .. } => "config_too_long",
            Broken::SizeOutOfRange { .. } => "config_size_out_of_range",
            Broken::NumberOutOfRange { .. } => "config_number_out_of_range",
            Broken::TooManyWaits { .. } => "config_too_many_waits",
            Broken::Zero => "config_zero",
            Broken::NotHostPort(_) => "config_not_host_port",
            Broken::NotNetworks(_) => "config_not_networks",
            Broken::NotUrl(_) => "config_not_url",
            Broken::NotFlag(_) => "config_not_flag",
            Broken::NotTransforms(_) => "config_not_transforms",
            Broken::BucketRequired => "config_bucket_required",
            Broken::NotEmail(_) => "config_not_email",
            Broken::FromRequired(_) => "config_from_required",
            Broken::NotSender(_) => "config_not_sender",
            Broken::NotTls(_) => "config_not_tls",
            Broken::NotPort(_) => "config_not_port",
            Broken::Empty => "config_empty",
            Broken::NotAgentWrites(_) => "config_not_agent_writes",
        }
    }
}

fn list(problems: &[Problem]) -> String {
    problems
        .iter()
        .map(Problem::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

impl Config {
    /// Reads `text` (the file's contents, `None` without a file) over the defaults, then
    /// the environment through `env`, and checks the result. Every problem is reported
    /// at once.
    pub fn load(
        text: Option<&str>,
        env: impl Fn(&str) -> Option<String>,
    ) -> Result<Config, ConfigError> {
        let mut file: File = match text {
            Some(text) => toml::from_str(text)?,
            None => File::default(),
        };
        let mut problems = Vec::new();
        // An empty variable counts as unset, as `HYPHA_MEDIA_URL=` did before.
        let env = |name: &str| env(name).filter(|v| !v.trim().is_empty());
        let from_env = apply_env(&mut file, &env, &mut problems);
        let config = check(&file, &mut problems);
        // A value the environment set is reported under the variable, which is what the
        // operator wrote.
        for problem in &mut problems {
            if let Some((_, var)) = from_env.iter().find(|(key, _)| *key == problem.key) {
                problem.key = var.to_string();
            }
        }
        // By key, so a section's problems read together (stable: a key's stay in order).
        problems.sort_by(|a, b| a.key.cmp(&b.key));
        if problems.is_empty() {
            Ok(config)
        } else {
            Err(ConfigError::Invalid(problems))
        }
    }

    /// The defaults alone.
    pub fn defaults() -> Config {
        let mut problems = Vec::new();
        check(&File::default(), &mut problems)
    }

    /// The settings in effect as TOML: the file with the environment applied, every key
    /// filled in (`hypha-host-axum config`).
    pub fn to_toml(&self) -> String {
        // Plain data with string keys: serializing can't fail.
        toml::to_string(&self.effective).unwrap_or_default()
    }
}

/// The JSON Schema of `hypha.config.toml`, pretty-printed with a final newline.
pub fn schema_json() -> String {
    let schema = schemars::schema_for!(File);
    let mut json = serde_json::to_string_pretty(&schema).unwrap_or_default();
    json.push('\n');
    json
}

/// The variables that win over the file (section 5). Each problem names the variable.
/// Returns the keys they set, each with its variable, so the checks' problems can name
/// it too.
fn apply_env(
    file: &mut File,
    env: &impl Fn(&str) -> Option<String>,
    problems: &mut Vec<Problem>,
) -> Vec<(&'static str, &'static str)> {
    let flag = |name: &str, problems: &mut Vec<Problem>| match env(name).as_deref() {
        None => None,
        Some("true") => Some(true),
        Some("false") => Some(false),
        Some(other) => {
            problems.push(Problem {
                key: name.to_string(),
                broken: Broken::NotFlag(other.to_string()),
            });
            None
        }
    };
    let mut set = Vec::new();
    if let Some(bind) = env("HYPHA_BIND") {
        file.server.bind = bind;
        set.push(("server.bind", "HYPHA_BIND"));
    } else if let Some(port) = env("PORT") {
        file.server.bind = format!("0.0.0.0:{port}");
        set.push(("server.bind", "PORT"));
    }
    if let Some(list) = env("HYPHA_TRUSTED_PROXIES") {
        file.server.trusted_proxies = list
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        set.push(("server.trusted_proxies", "HYPHA_TRUSTED_PROXIES"));
    }
    if let Some(url) = env("HYPHA_ADMIN_URL") {
        file.server.admin_url = Some(url);
        set.push(("server.admin_url", "HYPHA_ADMIN_URL"));
    }
    if let Some(url) = env("HYPHA_PUBLIC_URL") {
        file.server.public_url = Some(url);
        set.push(("server.public_url", "HYPHA_PUBLIC_URL"));
    }
    if let Some(allow) = flag("HYPHA_ALLOW_PRIVATE_CALLBACKS", problems) {
        file.outbound.allow_private = allow;
    }
    if let Some(url) = env("HYPHA_MEDIA_URL") {
        file.media.public_url = Some(url);
        set.push(("media.public_url", "HYPHA_MEDIA_URL"));
    }
    match env("HYPHA_AGENT_WRITES").as_deref() {
        None => {}
        Some("apply") => file.agents.writes = file::AgentWrites::Apply,
        Some("propose") => file.agents.writes = file::AgentWrites::Propose,
        Some("off") => file.agents.writes = file::AgentWrites::Off,
        Some(other) => problems.push(Problem {
            key: "HYPHA_AGENT_WRITES".to_string(),
            broken: Broken::NotAgentWrites(other.to_string()),
        }),
    }
    match env("HYPHA_MEDIA_TRANSFORMS").as_deref() {
        None => {}
        Some("none") => file.media.transforms = Transforms::None,
        Some("cloudflare") => file.media.transforms = Transforms::Cloudflare,
        Some(other) => problems.push(Problem {
            key: "HYPHA_MEDIA_TRANSFORMS".to_string(),
            broken: Broken::NotTransforms(other.to_string()),
        }),
    }
    if let Some(endpoint) = env("HYPHA_S3_ENDPOINT") {
        file.media.s3.endpoint = Some(endpoint);
        set.push(("media.s3.endpoint", "HYPHA_S3_ENDPOINT"));
    }
    if let Some(region) = env("HYPHA_S3_REGION") {
        file.media.s3.region = region;
    }
    if let Some(bucket) = env("HYPHA_S3_BUCKET") {
        file.media.s3.bucket = Some(bucket);
    }
    if let Some(create) = flag("HYPHA_S3_CREATE_BUCKET", problems) {
        file.media.s3.create_bucket = create;
    }
    // Email: what `.cargo/config.toml` points development at Mailpit with, and what a
    // container sets without a file.
    match env("HYPHA_EMAIL_SENDER").as_deref() {
        None => {}
        Some("log") => file.email.sender = EmailSender::Log,
        Some("smtp") => file.email.sender = EmailSender::Smtp,
        Some("resend") => file.email.sender = EmailSender::Resend,
        Some(other) => problems.push(Problem {
            key: "HYPHA_EMAIL_SENDER".to_string(),
            broken: Broken::NotSender(other.to_string()),
        }),
    }
    if let Some(from) = env("HYPHA_EMAIL_FROM") {
        file.email.from = Some(from);
        set.push(("email.from", "HYPHA_EMAIL_FROM"));
    }
    if let Some(host) = env("HYPHA_SMTP_HOST") {
        file.email.smtp.host = host;
        set.push(("email.smtp.host", "HYPHA_SMTP_HOST"));
    }
    if let Some(port) = env("HYPHA_SMTP_PORT") {
        match port.trim().parse::<u16>() {
            Ok(port) => {
                file.email.smtp.port = port;
                set.push(("email.smtp.port", "HYPHA_SMTP_PORT"));
            }
            Err(_) => problems.push(Problem {
                key: "HYPHA_SMTP_PORT".to_string(),
                broken: Broken::NotPort(port),
            }),
        }
    }
    match env("HYPHA_SMTP_TLS").as_deref() {
        None => {}
        Some("none") => file.email.smtp.tls = SmtpTls::None,
        Some("starttls") => file.email.smtp.tls = SmtpTls::Starttls,
        Some("tls") => file.email.smtp.tls = SmtpTls::Tls,
        Some(other) => problems.push(Problem {
            key: "HYPHA_SMTP_TLS".to_string(),
            broken: Broken::NotTls(other.to_string()),
        }),
    }
    set
}

const SECOND: Duration = Duration::from_secs(1);
const MINUTE: Duration = Duration::from_secs(60);
const HOUR: Duration = Duration::from_secs(3600);
const DAY: Duration = Duration::from_secs(24 * 3600);
/// The longest an S3 presigned URL may work.
const PRESIGN_MAX: Duration = Duration::from_secs(7 * 24 * 3600);
/// The longest a lifetime or a retry wait may be. Far enough that no store needs more,
/// near enough that every expiry stays a date clients can read.
const LONGEST: Duration = Duration::from_secs(365 * 24 * 3600);
/// The largest and smallest request body limits; files go to storage directly, so the
/// largest is generous. Values valid before stay valid (key names and accepted values are a
/// contract with operators): below 256 KiB, a product's longest description (64 KiB,
/// escaped in JSON) may be refused with `413 body_too_large`.
const BODY_MIN: u64 = 1 << 10;
const BODY_MAX: u64 = 64 << 20;
/// How long a run gets past a delivery's longest, to record what happened.
const RECORD_TIME: Duration = Duration::from_secs(15);
/// How long events stay past the last retry: room for jobs that were stopped a while.
const EVENTS_MARGIN: Duration = DAY;

/// Collects problems, each under its key.
struct Problems<'a>(&'a mut Vec<Problem>);

impl Problems<'_> {
    fn add(&mut self, key: &str, broken: Broken) {
        self.0.push(Problem {
            key: key.to_string(),
            broken,
        });
    }

    fn at_least(&mut self, key: &str, value: Dur, min: Duration) {
        self.longer_than(key, value, min, None);
    }

    /// At least `min`, which comes from the setting `because` (if any).
    fn longer_than(&mut self, key: &str, value: Dur, min: Duration, because: Option<&'static str>) {
        if value.0 < min {
            let min = Dur(min);
            self.add(
                key,
                Broken::TooShort {
                    value,
                    min,
                    because,
                },
            );
        }
    }

    fn at_most(&mut self, key: &str, value: Dur, max: Duration) {
        if value.0 > max {
            let max = Dur(max);
            self.add(key, Broken::TooLong { value, max });
        }
    }

    fn between(&mut self, key: &str, value: Dur, min: Duration, max: Duration) {
        self.at_least(key, value, min);
        self.at_most(key, value, max);
    }
}

/// Checks `file` (section 8) and converts it, adding a problem for each broken rule.
/// What it returns is only used when there are none.
fn check(file: &File, problems: &mut Vec<Problem>) -> Config {
    let mut p = Problems(problems);

    // Server.
    let server = &file.server;
    let port_ok = server
        .bind
        .rsplit_once(':')
        .is_some_and(|(host, port)| !host.is_empty() && port.parse::<u16>().is_ok());
    if !port_ok {
        p.add("server.bind", Broken::NotHostPort(server.bind.clone()));
    }
    let mut trusted_proxies = Vec::new();
    let mut bad_proxies = Vec::new();
    for entry in &server.trusted_proxies {
        match entry
            .parse::<IpNet>()
            .or_else(|_| entry.parse::<std::net::IpAddr>().map(IpNet::from))
        {
            Ok(net) => trusted_proxies.push(net),
            Err(_) => bad_proxies.push(entry.clone()),
        }
    }
    if !bad_proxies.is_empty() {
        p.add("server.trusted_proxies", Broken::NotNetworks(bad_proxies));
    }
    if !(BODY_MIN..=BODY_MAX).contains(&server.max_body.0) {
        let (min, max) = (Size(BODY_MIN), Size(BODY_MAX));
        let value = server.max_body;
        p.add(
            "server.max_body",
            Broken::SizeOutOfRange { value, min, max },
        );
    }
    let max_body = usize::try_from(server.max_body.0).unwrap_or(usize::MAX);
    p.at_least("server.shutdown_grace", server.shutdown_grace, SECOND);
    let admin_url = server
        .admin_url
        .as_deref()
        .map(|url| url.trim_end_matches('/').to_string());
    let public_url = server
        .public_url
        .as_deref()
        .map(|url| url.trim_end_matches('/').to_string());

    // Database.
    p.at_least(
        "database.pool_timeout",
        file.database.pool_timeout,
        Duration::from_millis(1),
    );
    if file.database.max_connections == Some(0) {
        p.add("database.max_connections", Broken::Zero);
    }

    // Agents.
    p.between(
        "agents.preview_timeout",
        file.agents.preview_timeout,
        Duration::from_millis(100),
        Duration::from_secs(30),
    );
    p.between(
        "agents.proposal_lifetime",
        file.agents.proposal_lifetime,
        Duration::from_secs(3600),
        Duration::from_secs(90 * 24 * 3600),
    );
    p.between(
        "agents.preview_token_lifetime",
        file.agents.preview_token_lifetime,
        Duration::from_secs(60),
        Duration::from_secs(24 * 3600),
    );

    // URLs.
    let media = &file.media;
    for (key, url) in [
        ("server.admin_url", &server.admin_url),
        ("server.public_url", &server.public_url),
        ("media.public_url", &media.public_url),
        ("media.s3.endpoint", &media.s3.endpoint),
    ] {
        if let Some(url) = url
            && !(url.starts_with("https://") || url.starts_with("http://"))
        {
            p.add(key, Broken::NotUrl(url.clone()));
        }
    }

    // Media.
    for (key, value) in [
        ("media.upload_expires", media.upload_expires),
        ("media.download_expires", media.download_expires),
    ] {
        // S3 signs URLs for 7 days at most.
        p.between(key, value, MINUTE, PRESIGN_MAX);
    }
    p.longer_than(
        "media.abandoned_after",
        media.abandoned_after,
        media.upload_expires.0,
        Some("media.upload_expires"),
    );
    if media.s3.endpoint.is_some() && media.s3.bucket.is_none() {
        p.add("media.s3.bucket", Broken::BucketRequired);
    }

    // Auth.
    let auth = &file.auth;
    for (key, value, min) in [
        ("auth.session_lifetime", auth.session_lifetime, MINUTE),
        ("auth.invite_lifetime", auth.invite_lifetime, MINUTE),
        (
            "auth.second_factor_window",
            auth.second_factor_window,
            SECOND,
        ),
        ("auth.second_factor_lock", auth.second_factor_lock, SECOND),
        (
            "auth.password_reset_lifetime",
            auth.password_reset_lifetime,
            SECOND,
        ),
        (
            "auth.password_reset_resend_after",
            auth.password_reset_resend_after,
            SECOND,
        ),
    ] {
        p.between(key, value, min, LONGEST);
    }
    let locked_accounts = auth
        .locked_accounts
        .iter()
        .filter_map(|a| {
            hypha_core::Email::new(a)
                .inspect_err(|_| p.add("auth.locked_accounts", Broken::NotEmail(a.clone())))
                .ok()
        })
        .collect();

    // Email.
    let email = &file.email;
    let from = match &email.from {
        Some(from) => hypha_core::Email::new(from)
            .inspect_err(|_| p.add("email.from", Broken::NotEmail(from.clone())))
            .ok(),
        None => {
            if email.sender != EmailSender::Log {
                p.add("email.from", Broken::FromRequired(email.sender.as_str()));
            }
            None
        }
    };
    let sender = match email.sender {
        EmailSender::Log => SenderConfig::Log,
        EmailSender::Resend => SenderConfig::Resend,
        EmailSender::Smtp => {
            let smtp = &email.smtp;
            if smtp.host.trim().is_empty() {
                p.add("email.smtp.host", Broken::Empty);
            }
            if smtp.port == 0 {
                p.add("email.smtp.port", Broken::Zero);
            }
            if smtp
                .username
                .as_deref()
                .is_some_and(|u| u.trim().is_empty())
            {
                p.add("email.smtp.username", Broken::Empty);
            }
            SenderConfig::Smtp(SmtpConfig {
                host: smtp.host.trim().to_string(),
                port: smtp.port,
                tls: smtp.tls,
                username: smtp.username.clone(),
            })
        }
    };
    let email = EmailConfig {
        sender,
        // A constant that passes `Email::new`; only used when there are no problems.
        from: from.unwrap_or_else(|| {
            hypha_core::Email::new(EmailConfig::LOG_FROM)
                .unwrap_or_else(|_| unreachable!("LOG_FROM is an address"))
        }),
    };

    // Webhooks.
    let hooks = &file.webhooks;
    for (i, wait) in hooks.retry.iter().enumerate() {
        p.between(&format!("webhooks.retry[{i}]"), *wait, SECOND, LONGEST);
    }
    if hooks.retry.len() > 20 {
        let count = hooks.retry.len();
        p.add("webhooks.retry", Broken::TooManyWaits { count, max: 20 });
    }
    p.between(
        "webhooks.retry_then_every",
        hooks.retry_then_every,
        SECOND,
        LONGEST,
    );
    let (first_key, first_wait) = match hooks.retry.first() {
        Some(wait) => ("webhooks.retry[0]", *wait),
        None => ("webhooks.retry_then_every", hooks.retry_then_every),
    };
    p.longer_than(
        "webhooks.give_up_after",
        hooks.give_up_after,
        first_wait.0,
        Some(first_key),
    );
    p.at_most("webhooks.give_up_after", hooks.give_up_after, LONGEST);

    // Jobs.
    let jobs = &file.jobs;
    for (key, value) in [
        ("jobs.dispatch.interval", jobs.dispatch.interval),
        ("jobs.prune.interval", jobs.prune.interval),
        ("jobs.media_sweep.interval", jobs.media_sweep.interval),
        ("jobs.search_sync.interval", jobs.search_sync.interval),
        (
            "jobs.send_notifications.interval",
            jobs.send_notifications.interval,
        ),
        ("jobs.prune.sessions_after", jobs.prune.sessions_after),
        (
            "jobs.prune.extension_calls_after",
            jobs.prune.extension_calls_after,
        ),
        ("jobs.prune.agent_calls_after", jobs.prune.agent_calls_after),
        ("jobs.idle_interval", jobs.idle_interval),
    ] {
        p.at_least(key, value, Duration::from_millis(1));
    }
    for (key, batch) in [
        ("jobs.dispatch.batch", jobs.dispatch.batch),
        ("jobs.search_sync.batch", jobs.search_sync.batch),
        (
            "jobs.send_notifications.batch",
            jobs.send_notifications.batch,
        ),
    ] {
        if !(1..=1_000).contains(&batch) {
            let value = u64::from(batch);
            p.add(
                key,
                Broken::NumberOutOfRange {
                    value,
                    min: 1,
                    max: 1_000,
                },
            );
        }
    }
    if file.customers.erasure_delay.0 < hypha_privacy::PrivacySettings::MIN_ERASURE_DELAY {
        p.add(
            "customers.erasure_delay",
            Broken::TooShort {
                value: file.customers.erasure_delay,
                min: Dur(hypha_privacy::PrivacySettings::MIN_ERASURE_DELAY),
                because: None,
            },
        );
    }
    let customers = &file.customers;
    p.at_least(
        "customers.session_lifetime",
        customers.session_lifetime,
        MINUTE,
    );
    p.at_least(
        "customers.login_code_lifetime",
        customers.login_code_lifetime,
        MINUTE,
    );
    p.at_least(
        "customers.login_code_resend_after",
        customers.login_code_resend_after,
        SECOND,
    );
    p.longer_than(
        "customers.login_code_lifetime",
        customers.login_code_lifetime,
        customers.login_code_resend_after.0,
        Some("customers.login_code_resend_after"),
    );
    let max_depth = file.categories.max_depth;
    if !(1..=CategorySettings::MAX_DEPTH_LIMIT).contains(&max_depth) {
        p.add(
            "categories.max_depth",
            Broken::NumberOutOfRange {
                value: u64::from(max_depth),
                min: 1,
                max: u64::from(CategorySettings::MAX_DEPTH_LIMIT),
            },
        );
    }
    // Long enough to rebuild a batch, so a lease doesn't end while its runner works.
    p.at_least(
        "jobs.search_sync.lease",
        jobs.search_sync.lease,
        Duration::from_secs(10),
    );
    // A lease must hold one send, with room to record it.
    p.at_least(
        "jobs.send_notifications.lease",
        jobs.send_notifications.lease,
        LONGEST_SEND.saturating_add(Duration::from_secs(5)),
    );
    let sends = &jobs.send_notifications;
    for (i, wait) in sends.retry.iter().enumerate() {
        p.between(
            &format!("jobs.send_notifications.retry[{i}]"),
            *wait,
            SECOND,
            LONGEST,
        );
    }
    if sends.retry.len() > 20 {
        let count = sends.retry.len();
        p.add(
            "jobs.send_notifications.retry",
            Broken::TooManyWaits { count, max: 20 },
        );
    }
    // Pruning an event deletes its deliveries, so none may still be due: events outlive
    // the last retry.
    p.longer_than(
        "jobs.prune.events_after",
        jobs.prune.events_after,
        hooks.give_up_after.0.saturating_add(EVENTS_MARGIN),
        Some("webhooks.give_up_after plus a day"),
    );
    p.at_least(
        "jobs.prune.idempotency_keys_after",
        jobs.prune.idempotency_keys_after,
        HOUR,
    );
    // A lease must hold at least one delivery's longest run, with room to record it.
    p.at_least(
        "jobs.dispatch.lease",
        jobs.dispatch.lease,
        LONGEST_RUN.saturating_add(RECORD_TIME),
    );

    // Rate limits.
    for (name, quota) in file.rate_limits.all() {
        p.at_least(
            &format!("rate_limits.{name}.per"),
            quota.per,
            Duration::from_millis(1),
        );
        if quota.burst == 0 {
            p.add(&format!("rate_limits.{name}.burst"), Broken::Zero);
        }
    }

    let job = |enabled: bool, interval: Dur| JobConfig {
        enabled: jobs.enabled && enabled,
        interval: interval.0,
    };
    Config {
        server: ServerConfig {
            bind: server.bind.clone(),
            trusted_proxies,
            admin_url,
            public_url,
            max_body,
            shutdown_grace: server.shutdown_grace.0,
        },
        database: DatabaseConfig {
            statement_cache: file.database.statement_cache,
            pool_timeout: file.database.pool_timeout.0,
            max_connections: file
                .database
                .max_connections
                .map(|n| usize::try_from(n).unwrap_or(usize::MAX)),
        },
        allow_private_callbacks: file.outbound.allow_private,
        media: MediaConfig {
            s3: match (&media.s3.endpoint, &media.s3.bucket) {
                (Some(endpoint), Some(bucket)) => Some(S3Config {
                    endpoint: endpoint.clone(),
                    region: media.s3.region.clone(),
                    bucket: bucket.clone(),
                    create_bucket: media.s3.create_bucket,
                }),
                _ => None,
            },
            public_url: media.public_url.clone(),
            transforms: media.transforms,
            settings: MediaSettings {
                upload_expires: media.upload_expires.0,
                download_expires: media.download_expires.0,
                abandoned_after: media.abandoned_after.0,
            },
        },
        auth: AuthSettings {
            session_lifetime: auth.session_lifetime.0,
            second_factor_window: auth.second_factor_window.0,
            second_factor_lock: auth.second_factor_lock.0,
            password_reset_lifetime: auth.password_reset_lifetime.0,
            password_reset_resend_after: auth.password_reset_resend_after.0,
            invite_lifetime: auth.invite_lifetime.0,
        },
        staff: hypha_users::StaffSettings { locked_accounts },
        email,
        categories: CategorySettings { max_depth },
        privacy: hypha_privacy::PrivacySettings {
            erasure_delay: file.customers.erasure_delay.0,
        },
        accounts: hypha_customer_accounts::AccountSettings {
            session_lifetime: file.customers.session_lifetime.0,
            login_code_lifetime: file.customers.login_code_lifetime.0,
            login_code_resend_after: file.customers.login_code_resend_after.0,
        },
        dispatch: DispatchSettings {
            batch: jobs.dispatch.batch,
            lease: jobs.dispatch.lease.0,
            retry: RetrySchedule {
                backoff: hooks.retry.iter().map(|d| d.0).collect(),
                then_every: hooks.retry_then_every.0,
                give_up_after: hooks.give_up_after.0,
            },
            keep_events: jobs.prune.events_after.0,
            keep_idempotency_keys: jobs.prune.idempotency_keys_after.0,
            keep_ended_sessions: jobs.prune.sessions_after.0,
        },
        search_sync: SyncSettings {
            batch: jobs.search_sync.batch,
            lease: jobs.search_sync.lease.0,
        },
        send_notifications: SendSettings {
            batch: jobs.send_notifications.batch,
            lease: jobs.send_notifications.lease.0,
            retry: jobs.send_notifications.retry.iter().map(|d| d.0).collect(),
        },
        extensions: ExtensionSettings {
            keep_calls: jobs.prune.extension_calls_after.0,
        },
        jobs: JobsConfig {
            idle_interval: jobs.idle_interval.0,
            dispatch: job(jobs.dispatch.enabled, jobs.dispatch.interval),
            prune: job(jobs.prune.enabled, jobs.prune.interval),
            media_sweep: job(jobs.media_sweep.enabled, jobs.media_sweep.interval),
            search_sync: job(jobs.search_sync.enabled, jobs.search_sync.interval),
            send_notifications: job(
                jobs.send_notifications.enabled,
                jobs.send_notifications.interval,
            ),
        },
        rate_limits: file.rate_limits.clone(),
        agents: AgentsConfig {
            enabled: file.agents.enabled,
            writes: file.agents.writes.cap(),
            preview_timeout: file.agents.preview_timeout.0,
            preview_token_lifetime: file.agents.preview_token_lifetime.0,
            settings: hypha_agents::AgentSettings {
                proposal_lifetime: file.agents.proposal_lifetime.0,
                keep_calls: jobs.prune.agent_calls_after.0,
            },
        },
        spores: file.spores.clone(),
        effective: file.clone(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn no_env(_: &str) -> Option<String> {
        None
    }

    fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let vars: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |name| vars.get(name).cloned()
    }

    fn load(text: &str) -> Result<Config, ConfigError> {
        Config::load(Some(text), no_env)
    }

    fn problems(result: Result<Config, ConfigError>) -> Vec<String> {
        match result {
            Err(ConfigError::Invalid(problems)) => {
                problems.iter().map(Problem::to_string).collect()
            }
            other => panic!("expected problems, got {other:?}"),
        }
    }

    #[test]
    fn no_file_means_every_default() {
        let config = Config::load(None, no_env).unwrap();
        assert_eq!(config, Config::defaults());
        assert_eq!(config.auth, AuthSettings::default());
        assert_eq!(config.dispatch, DispatchSettings::default());
        assert_eq!(config.media.settings, MediaSettings::default());
        assert_eq!(config.media.s3, None);
        assert_eq!(config.server.bind, "127.0.0.1:3000");
        assert_eq!(config.server.max_body, 2 * 1024 * 1024);
        assert_eq!(config.jobs.dispatch.interval, Duration::from_secs(2));
        assert!(config.jobs.prune.enabled);
        assert_eq!(config.rate_limits, RateLimits::default());
    }

    #[test]
    fn an_empty_file_is_the_defaults_too() {
        assert_eq!(load(""), Ok(Config::defaults()));
    }

    #[test]
    fn the_file_sets_what_it_names_and_leaves_the_rest() {
        let config = load(
            r#"
            [auth]
            session_lifetime = "12h"

            [webhooks]
            retry = ["10s", "1m"]
            give_up_after = "1h"

            [jobs.dispatch]
            batch = 25
            lease = "5m"

            [rate_limits]
            login_ip = { burst = 3, per = "1m" }
            "#,
        )
        .unwrap();
        assert_eq!(config.auth.session_lifetime, 12 * HOUR);
        assert_eq!(
            config.auth.invite_lifetime,
            AuthSettings::default().invite_lifetime
        );
        assert_eq!(
            config.dispatch.retry.backoff,
            [Duration::from_secs(10), MINUTE]
        );
        assert_eq!(config.dispatch.retry.give_up_after, HOUR);
        assert_eq!(config.dispatch.batch, 25);
        assert_eq!(config.dispatch.lease, 5 * MINUTE);
        assert_eq!(
            config.rate_limits.login_ip,
            Quota {
                burst: 3,
                per: Dur(MINUTE)
            }
        );
        assert_eq!(config.rate_limits.mfa_ip, RateLimits::default().mfa_ip);
    }

    #[test]
    fn an_unknown_key_or_a_wrong_value_names_where() {
        let Err(ConfigError::Parse(e)) = load("[jobs.dispatch]\nintervall = \"2s\"") else {
            panic!("a typo must fail");
        };
        let message = e.to_string();
        assert!(message.contains("intervall"), "{message}");
        assert!(message.contains("line 2"), "{message}");
        let Err(ConfigError::Parse(e)) = load("[auth]\nsession_lifetime = \"7 days\"") else {
            panic!("a bad duration must fail");
        };
        let message = e.to_string();
        assert!(message.contains("7 days"), "{message}");
        // Secrets have no key, so naming one is an unknown key.
        assert!(matches!(
            load("[database]\nurl = \"postgres://x\""),
            Err(ConfigError::Parse(_))
        ));
    }

    #[test]
    fn every_broken_rule_is_reported_at_once() {
        let found = problems(load(
            r#"
            [server]
            bind = "3000"
            trusted_proxies = ["10.0.0.0/8", "nope"]

            [jobs.dispatch]
            batch = 0
            lease = "30s"

            [jobs.prune]
            idempotency_keys_after = "10m"

            [media]
            upload_expires = "8d"
            abandoned_after = "1h"

            [media.s3]
            endpoint = "minio:9000"

            [rate_limits]
            cart_create_ip = { burst = 0, per = "0s" }
            "#,
        ));
        assert_eq!(
            found,
            [
                "jobs.dispatch.batch: is 0, 1 to 1000",
                "jobs.dispatch.lease: is 30s, at least 1m is needed",
                "jobs.prune.idempotency_keys_after: is 10m, at least 1h is needed",
                "media.abandoned_after: is 1h, at least 8d is needed (media.upload_expires)",
                "media.s3.bucket: is required with an endpoint (or HYPHA_S3_BUCKET)",
                "media.s3.endpoint: \"minio:9000\" isn't an http:// or https:// URL",
                "media.upload_expires: is 8d, at most 7d",
                "rate_limits.cart_create_ip.burst: is at least 1",
                "rate_limits.cart_create_ip.per: is 0s, at least 1ms is needed",
                "server.bind: \"3000\" isn't host:port, like \"0.0.0.0:3000\"",
                "server.trusted_proxies: not CIDR ranges or addresses: [\"nope\"]",
            ]
        );
    }

    #[test]
    fn events_outlive_the_last_retry_by_a_day() {
        // The prune deletes an event's deliveries too, so none may still be due.
        let found = problems(load("[jobs.prune]\nevents_after = \"1d\""));
        assert_eq!(
            found,
            ["jobs.prune.events_after: is 1d, at least 4d is needed \
              (webhooks.give_up_after plus a day)"]
        );
        assert!(load("[jobs.prune]\nevents_after = \"4d\"").is_ok());
        let found = problems(load(
            "[webhooks]\ngive_up_after = \"10d\"\n[jobs.prune]\nevents_after = \"10d\"",
        ));
        assert_eq!(
            found,
            ["jobs.prune.events_after: is 10d, at least 11d is needed \
              (webhooks.give_up_after plus a day)"]
        );
    }

    #[test]
    fn locked_accounts_are_login_emails() {
        let config = load(
            r#"
            [auth]
            locked_accounts = ["demo@example.com"]
            "#,
        )
        .unwrap();
        assert_eq!(
            config.staff.locked_accounts,
            [hypha_core::Email::new("demo@example.com").unwrap()]
        );
        assert_eq!(
            problems(load(
                r#"
                [auth]
                locked_accounts = ["demo"]
                "#,
            )),
            ["auth.locked_accounts: \"demo\" isn't an email address"]
        );
    }

    #[test]
    fn lifetimes_and_waits_are_at_most_a_year() {
        let found = problems(load(
            r#"
            [auth]
            session_lifetime = "3000000d"
            password_reset_resend_after = "366d"

            [webhooks]
            retry = ["400d"]
            give_up_after = "400d"

            [jobs.prune]
            events_after = "500d"
            "#,
        ));
        assert_eq!(
            found,
            [
                "auth.password_reset_resend_after: is 366d, at most 365d",
                "auth.session_lifetime: is 3000000d, at most 365d",
                "webhooks.give_up_after: is 400d, at most 365d",
                "webhooks.retry[0]: is 400d, at most 365d",
            ]
        );
        assert!(load("[auth]\nsession_lifetime = \"365d\"").is_ok());
    }

    #[test]
    fn sending_notifications_has_a_batch_a_lease_and_retries() {
        let config = load("").unwrap();
        assert_eq!(config.send_notifications, SendSettings::default());
        assert!(config.jobs.send_notifications.enabled);
        let changed =
            load("[jobs.send_notifications]\nbatch = 5\nlease = \"1m\"\nretry = [\"30s\"]")
                .unwrap();
        assert_eq!(
            changed.send_notifications,
            SendSettings {
                batch: 5,
                lease: Duration::from_secs(60),
                retry: vec![Duration::from_secs(30)],
            }
        );
        assert_eq!(
            problems(load(
                "[jobs.send_notifications]\nbatch = 0\nlease = \"10s\"\nretry = [\"0s\"]"
            )),
            [
                "jobs.send_notifications.batch: is 0, 1 to 1000",
                "jobs.send_notifications.lease: is 10s, at least 20s is needed",
                "jobs.send_notifications.retry[0]: is 0s, at least 1s is needed",
            ]
        );
        assert!(
            !load("[jobs]\nenabled = false")
                .unwrap()
                .jobs
                .send_notifications
                .enabled
        );
    }

    #[test]
    fn search_sync_has_a_batch_and_a_lease() {
        let config = load("").unwrap();
        assert_eq!(config.search_sync, SyncSettings::default());
        assert!(config.jobs.search_sync.enabled);
        assert_eq!(
            problems(load("[jobs.search_sync]\nbatch = 0\nlease = \"5s\"")),
            [
                "jobs.search_sync.batch: is 0, 1 to 1000",
                "jobs.search_sync.lease: is 5s, at least 10s is needed",
            ]
        );
    }

    #[test]
    fn categories_nest_1_to_10_levels() {
        let config = load("").unwrap();
        assert_eq!(config.categories.max_depth, 5);
        assert_eq!(
            load("[categories]\nmax_depth = 10")
                .unwrap()
                .categories
                .max_depth,
            10
        );
        assert_eq!(
            problems(load("[categories]\nmax_depth = 0")),
            ["categories.max_depth: is 0, 1 to 10"]
        );
        assert_eq!(
            problems(load("[categories]\nmax_depth = 11")),
            ["categories.max_depth: is 11, 1 to 10"]
        );
    }

    #[test]
    fn the_body_limit_is_1kib_to_64mib() {
        for (value, ok) in [
            ("0B", false),
            ("1023B", false),
            ("1KiB", true),
            ("64MiB", true),
            ("67108865B", false),
            ("4GiB", false),
        ] {
            let result = load(&format!("[server]\nmax_body = \"{value}\""));
            if ok {
                assert!(result.is_ok(), "{value}");
            } else {
                assert_eq!(
                    problems(result),
                    [format!(
                        "server.max_body: is {}, 1KiB to 64MiB",
                        value.parse::<Size>().unwrap()
                    )],
                    "{value}"
                );
            }
        }
    }

    #[test]
    fn erasure_delays_urls_and_connections_are_checked() {
        for (text, problem) in [
            (
                "[customers]\nerasure_delay = \"59m\"",
                "customers.erasure_delay: is 59m, at least 1h is needed",
            ),
            (
                "[server]\nadmin_url = \"admin.example\"",
                "server.admin_url: \"admin.example\" isn't an http:// or https:// URL",
            ),
            (
                "[database]\nmax_connections = 0",
                "database.max_connections: is at least 1",
            ),
        ] {
            assert_eq!(problems(load(text)), [problem], "{text}");
        }
        assert!(load("[customers]\nerasure_delay = \"1h\"").is_ok());
    }

    #[test]
    fn a_previews_timeout_is_a_tenth_of_a_second_to_half_a_minute() {
        assert_eq!(
            problems(load("[agents]\npreview_timeout = \"99ms\"")),
            ["agents.preview_timeout: is 99ms, at least 100ms is needed"]
        );
        assert_eq!(
            problems(load("[agents]\npreview_timeout = \"31s\"")).len(),
            1
        );
        assert_eq!(
            load("[agents]\npreview_timeout = \"100ms\"")
                .unwrap()
                .agents
                .preview_timeout,
            Duration::from_millis(100)
        );
        assert_eq!(
            load("").unwrap().agents.preview_timeout,
            Duration::from_secs(5)
        );
    }

    #[test]
    fn stopping_waits_at_least_a_second() {
        assert_eq!(
            problems(load("[server]\nshutdown_grace = \"0s\"")),
            ["server.shutdown_grace: is 0s, at least 1s is needed"]
        );
        assert!(load("[server]\nshutdown_grace = \"1s\"").is_ok());
    }

    #[test]
    fn a_lease_holds_the_longest_run_and_time_to_record_it() {
        // `LONGEST_RUN` (45s) plus 15 seconds.
        assert_eq!(
            problems(load("[jobs.dispatch]\nlease = \"59s\"")),
            ["jobs.dispatch.lease: is 59s, at least 1m is needed"]
        );
        assert!(load("[jobs.dispatch]\nlease = \"60s\"").is_ok());
    }

    #[test]
    fn a_bad_value_from_the_environment_names_the_variable() {
        let found = problems(Config::load(
            Some("[server]\nbind = \"127.0.0.1:3000\""),
            env(&[
                ("HYPHA_TRUSTED_PROXIES", "10.0.0.0/8,nope"),
                ("PORT", "eighty"),
                ("HYPHA_ADMIN_URL", "admin.example.com"),
                ("HYPHA_S3_ENDPOINT", "minio:9000"),
                ("HYPHA_S3_BUCKET", "hypha"),
            ]),
        ));
        assert_eq!(
            found,
            [
                "HYPHA_ADMIN_URL: \"admin.example.com\" isn't an http:// or https:// URL",
                "HYPHA_S3_ENDPOINT: \"minio:9000\" isn't an http:// or https:// URL",
                "HYPHA_TRUSTED_PROXIES: not CIDR ranges or addresses: [\"nope\"]",
                "PORT: \"0.0.0.0:eighty\" isn't host:port, like \"0.0.0.0:3000\"",
            ]
        );
        // From the file, the key.
        assert_eq!(
            problems(load("[server]\nbind = \"3000\"")),
            ["server.bind: \"3000\" isn't host:port, like \"0.0.0.0:3000\""]
        );
    }

    #[test]
    fn the_environment_wins_over_the_file() {
        let file = r#"
            [server]
            bind = "127.0.0.1:4000"
            trusted_proxies = ["10.0.0.1"]

            [outbound]
            allow_private = true

            [media]
            transforms = "cloudflare"
            "#;
        let config = Config::load(
            Some(file),
            env(&[
                ("PORT", "8080"),
                ("HYPHA_TRUSTED_PROXIES", "10.0.0.0/8, 192.168.1.1"),
                ("HYPHA_ALLOW_PRIVATE_CALLBACKS", "false"),
                ("HYPHA_MEDIA_TRANSFORMS", "none"),
                ("HYPHA_S3_ENDPOINT", "http://localhost:9000"),
                ("HYPHA_S3_BUCKET", "hypha"),
                ("HYPHA_ADMIN_URL", "https://admin.example.com/"),
                ("HYPHA_PUBLIC_URL", "https://api.example.com/"),
            ]),
        )
        .unwrap();
        assert_eq!(config.server.bind, "0.0.0.0:8080");
        assert_eq!(
            config.server.public_url.as_deref(),
            Some("https://api.example.com")
        );
        assert_eq!(
            config.server.trusted_proxies,
            [
                "10.0.0.0/8".parse::<IpNet>().unwrap(),
                "192.168.1.1/32".parse().unwrap()
            ]
        );
        assert!(!config.allow_private_callbacks);
        assert_eq!(config.media.transforms, Transforms::None);
        assert_eq!(
            config.media.s3,
            Some(S3Config {
                endpoint: "http://localhost:9000".to_string(),
                region: "auto".to_string(),
                bucket: "hypha".to_string(),
                create_bucket: false,
            })
        );
        assert_eq!(
            config.server.admin_url.as_deref(),
            Some("https://admin.example.com")
        );
    }

    #[test]
    fn hypha_bind_wins_over_port_and_empty_variables_are_unset() {
        let both = env(&[("HYPHA_BIND", "[::]:9000"), ("PORT", "8080")]);
        assert_eq!(Config::load(None, both).unwrap().server.bind, "[::]:9000");
        let empty = env(&[("HYPHA_MEDIA_URL", ""), ("HYPHA_BIND", " ")]);
        let config = Config::load(None, empty).unwrap();
        assert_eq!(config.media.public_url, None);
        assert_eq!(config.server.bind, "127.0.0.1:3000");
    }

    #[test]
    fn agents_writes_are_capped_by_the_file_or_the_environment() {
        use hypha_agents::WritesCap;
        assert_eq!(load("").unwrap().agents.writes, WritesCap::Apply);
        assert!(load("").unwrap().agents.enabled);
        let config = load("[agents]\nwrites = \"propose\"\nenabled = false").unwrap();
        assert_eq!(
            (config.agents.writes, config.agents.enabled),
            (WritesCap::Propose, false)
        );
        let config = Config::load(
            Some("[agents]\nwrites = \"propose\""),
            env(&[("HYPHA_AGENT_WRITES", "off")]),
        )
        .unwrap();
        assert_eq!(config.agents.writes, WritesCap::Off, "the environment wins");
        assert_eq!(
            problems(Config::load(None, env(&[("HYPHA_AGENT_WRITES", "all")]))),
            ["HYPHA_AGENT_WRITES: is `apply`, `propose` or `off`, not \"all\""]
        );
        assert!(load("[agents]\nwrites = \"all\"").is_err());
    }

    #[test]
    fn agents_lifetimes_have_bounds() {
        let config = load("").unwrap();
        assert_eq!(
            config.agents.preview_token_lifetime,
            Duration::from_secs(900)
        );
        assert_eq!(
            config.agents.settings,
            hypha_agents::AgentSettings::default()
        );
        let config =
            load("[agents]\nproposal_lifetime = \"2d\"\n[jobs.prune]\nagent_calls_after = \"3d\"")
                .unwrap();
        assert_eq!(
            config.agents.settings,
            hypha_agents::AgentSettings {
                proposal_lifetime: Duration::from_secs(2 * 86400),
                keep_calls: Duration::from_secs(3 * 86400),
            }
        );
        assert_eq!(
            problems(load(
                "[agents]\npreview_token_lifetime = \"59s\"\nproposal_lifetime = \"91d\""
            )),
            [
                "agents.preview_token_lifetime: is 59s, at least 1m is needed",
                "agents.proposal_lifetime: is 91d, at most 90d",
            ]
        );
    }

    #[test]
    fn a_flag_is_true_or_false() {
        let found = problems(Config::load(
            None,
            env(&[
                ("HYPHA_ALLOW_PRIVATE_CALLBACKS", "yes"),
                ("HYPHA_MEDIA_TRANSFORMS", "imgix"),
            ]),
        ));
        assert_eq!(
            found,
            [
                "HYPHA_ALLOW_PRIVATE_CALLBACKS: is true or false, not \"yes\"",
                "HYPHA_MEDIA_TRANSFORMS: is `cloudflare` or `none`, not \"imgix\"",
            ]
        );
    }

    #[test]
    fn email_is_logged_by_default_from_a_placeholder() {
        let config = Config::defaults();
        assert_eq!(config.email.sender, SenderConfig::Log);
        assert_eq!(config.email.from.as_str(), EmailConfig::LOG_FROM);
        assert_eq!(config.email.sender.name(), "log");
    }

    #[test]
    fn smtp_takes_its_server_and_the_from_address() {
        let config = load(
            r#"
            [email]
            sender = "smtp"
            from = "Shop@Mail.Example.com"

            [email.smtp]
            host = "smtp.example.com"
            port = 465
            tls = "tls"
            username = "apikey"
            "#,
        )
        .unwrap();
        assert_eq!(config.email.from.as_str(), "shop@mail.example.com");
        assert_eq!(
            config.email.sender,
            SenderConfig::Smtp(SmtpConfig {
                host: "smtp.example.com".to_string(),
                port: 465,
                tls: SmtpTls::Tls,
                username: Some("apikey".to_string()),
            })
        );
        let smtp = load("[email]\nsender = \"smtp\"\nfrom = \"a@b.co\"").unwrap();
        assert_eq!(
            smtp.email.sender,
            SenderConfig::Smtp(SmtpConfig {
                host: "localhost".to_string(),
                port: 587,
                tls: SmtpTls::Starttls,
                username: None,
            })
        );
    }

    #[test]
    fn a_sender_other_than_the_log_needs_a_from_address() {
        assert_eq!(
            problems(load("[email]\nsender = \"resend\"")),
            ["email.from: is required with sender `resend`"]
        );
        assert_eq!(
            problems(load("[email]\nsender = \"smtp\"")),
            ["email.from: is required with sender `smtp`"]
        );
        let resend = load("[email]\nsender = \"resend\"\nfrom = \"shop@example.com\"");
        assert_eq!(resend.unwrap().email.sender, SenderConfig::Resend);
    }

    #[test]
    fn email_values_are_checked() {
        let found = problems(load(
            r#"
            [email]
            sender = "smtp"
            from = "not an address"

            [email.smtp]
            host = " "
            port = 0
            username = ""
            "#,
        ));
        assert_eq!(
            found,
            [
                "email.from: \"not an address\" isn't an email address",
                "email.smtp.host: can't be empty",
                "email.smtp.port: is at least 1",
                "email.smtp.username: can't be empty",
            ]
        );
        assert!(matches!(
            load("[email]\nsender = \"sendgrid\""),
            Err(ConfigError::Parse(_))
        ));
    }

    #[test]
    fn email_variables_win_over_the_file_and_name_themselves() {
        let config = Config::load(
            Some("[email]\nsender = \"resend\"\nfrom = \"shop@example.com\""),
            env(&[
                ("HYPHA_EMAIL_SENDER", "smtp"),
                ("HYPHA_EMAIL_FROM", "dev@hypha.localhost"),
                ("HYPHA_SMTP_HOST", "mailpit"),
                ("HYPHA_SMTP_PORT", "1025"),
                ("HYPHA_SMTP_TLS", "none"),
            ]),
        )
        .unwrap();
        assert_eq!(config.email.from.as_str(), "dev@hypha.localhost");
        assert_eq!(
            config.email.sender,
            SenderConfig::Smtp(SmtpConfig {
                host: "mailpit".to_string(),
                port: 1025,
                tls: SmtpTls::None,
                username: None,
            })
        );
        let found = problems(Config::load(
            None,
            env(&[
                ("HYPHA_EMAIL_SENDER", "postmark"),
                ("HYPHA_EMAIL_FROM", "nope"),
                ("HYPHA_SMTP_PORT", "70000"),
                ("HYPHA_SMTP_TLS", "ssl"),
            ]),
        ));
        assert_eq!(
            found,
            [
                "HYPHA_EMAIL_FROM: \"nope\" isn't an email address",
                "HYPHA_EMAIL_SENDER: is `log`, `smtp` or `resend`, not \"postmark\"",
                "HYPHA_SMTP_PORT: \"70000\" isn't a port, 1 to 65535",
                "HYPHA_SMTP_TLS: is `none`, `starttls` or `tls`, not \"ssl\"",
            ]
        );
    }

    #[test]
    fn jobs_off_turns_every_job_off() {
        let config = load("[jobs]\nenabled = false").unwrap();
        assert!(!config.jobs.dispatch.enabled);
        assert!(!config.jobs.prune.enabled);
        assert!(!config.jobs.media_sweep.enabled);
        let config = load("[jobs.prune]\nenabled = false").unwrap();
        assert!(config.jobs.dispatch.enabled);
        assert!(!config.jobs.prune.enabled);
    }

    #[test]
    fn the_printed_settings_read_back_the_same() {
        let config = Config::load(
            Some("[auth]\nsession_lifetime = \"90s\"\n[webhooks]\nretry = []"),
            env(&[("HYPHA_ADMIN_URL", "https://admin.example.com")]),
        )
        .unwrap();
        let printed = config.to_toml();
        assert!(printed.contains("session_lifetime = \"90s\""), "{printed}");
        assert_eq!(Config::load(Some(&printed), no_env), Ok(config));
    }

    #[test]
    fn each_spores_section_is_kept_as_written_and_printed() {
        let text = "[spores.test]\napi_base = \"http://localhost:9\"\nretries = 3\n";
        let config = Config::load(Some(text), no_env).unwrap();
        let section = &config.spores["test"];
        assert_eq!(section["api_base"].as_str(), Some("http://localhost:9"));
        assert_eq!(section["retries"].as_integer(), Some(3));
        let printed = config.to_toml();
        assert!(printed.contains("[spores.test]"), "{printed}");
        assert_eq!(Config::load(Some(&printed), no_env), Ok(config));
        assert!(Config::defaults().spores.is_empty());
        assert!(!Config::defaults().to_toml().contains("spores"));
    }

    #[test]
    fn the_committed_schema_is_current() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../hypha.config.schema.json"
        );
        let committed = std::fs::read_to_string(path).unwrap_or_default();
        assert!(
            committed == schema_json(),
            "hypha.config.schema.json is stale: run `cargo run -p hypha-host-axum -- config schema`"
        );
    }
}
