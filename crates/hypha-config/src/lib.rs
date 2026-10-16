//! Hypha's configuration (design note 0017): `hypha.config.toml`, the environment
//! variables that win over it, the checks, and the settings each crate takes.
//!
//! It parses a string and reads the environment through a function it's given, so it
//! builds for wasm: the native host passes the file's text and `std::env::var`, a
//! Workers host its bindings, tests a map. Only hosts depend on it (CF3).
//!
//! ```text
//! defaults ─► hypha.config.toml ─► environment ─► checks ─► Config
//! ```

pub mod file;
pub mod units;

use core::time::Duration;

use hypha_auth::AuthSettings;
use hypha_core::ErrorCode;
use hypha_dispatch::{DispatchSettings, LONGEST_RUN, RetrySchedule};
use hypha_media::MediaSettings;
use ipnet::IpNet;
use thiserror::Error;

pub use file::{File, Quota, RateLimits, Transforms};
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
    pub dispatch: DispatchSettings,
    pub jobs: JobsConfig,
    pub rate_limits: RateLimits,
    /// The file with the environment applied, for `Config::to_toml`.
    effective: File,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerConfig {
    /// `host:port`.
    pub bind: String,
    pub trusted_proxies: Vec<IpNet>,
    /// Without a trailing `/`.
    pub admin_url: Option<String>,
    pub max_body: usize,
    pub shutdown_grace: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseConfig {
    pub pool_timeout: Duration,
    pub max_connections: Option<usize>,
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

/// Each job's switch (already off when `jobs.enabled` is) and interval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobsConfig {
    pub dispatch: JobConfig,
    pub prune: JobConfig,
    pub media_sweep: JobConfig,
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

/// The JSON Schema of `hypha.config.toml` (CF7), pretty-printed with a final newline.
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
    if let Some(allow) = flag("HYPHA_ALLOW_PRIVATE_CALLBACKS", problems) {
        file.outbound.allow_private = allow;
    }
    if let Some(url) = env("HYPHA_MEDIA_URL") {
        file.media.public_url = Some(url);
        set.push(("media.public_url", "HYPHA_MEDIA_URL"));
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
/// The largest and smallest request body limits: every route's JSON fits in the
/// smallest, and files go to storage directly, so the largest is generous.
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

    // Database.
    p.at_least(
        "database.pool_timeout",
        file.database.pool_timeout,
        Duration::from_millis(1),
    );
    if file.database.max_connections == Some(0) {
        p.add("database.max_connections", Broken::Zero);
    }

    // URLs.
    let media = &file.media;
    for (key, url) in [
        ("server.admin_url", &server.admin_url),
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
        ("jobs.prune.sessions_after", jobs.prune.sessions_after),
    ] {
        p.at_least(key, value, Duration::from_millis(1));
    }
    if !(1..=1_000).contains(&jobs.dispatch.batch) {
        let value = u64::from(jobs.dispatch.batch);
        p.add(
            "jobs.dispatch.batch",
            Broken::NumberOutOfRange {
                value,
                min: 1,
                max: 1_000,
            },
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
            max_body,
            shutdown_grace: server.shutdown_grace.0,
        },
        database: DatabaseConfig {
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
        jobs: JobsConfig {
            dispatch: job(jobs.dispatch.enabled, jobs.dispatch.interval),
            prune: job(jobs.prune.enabled, jobs.prune.interval),
            media_sweep: job(jobs.media_sweep.enabled, jobs.media_sweep.interval),
        },
        rate_limits: file.rate_limits.clone(),
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
            ]),
        )
        .unwrap();
        assert_eq!(config.server.bind, "0.0.0.0:8080");
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
