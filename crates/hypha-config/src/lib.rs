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
pub use units::{Dur, Size};

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
    Parse(String),
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub key: String,
    pub message: String,
}

fn list(problems: &[Problem]) -> String {
    problems
        .iter()
        .map(|p| format!("{}: {}", p.key, p.message))
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
            Some(text) => toml::from_str(text).map_err(|e| ConfigError::Parse(e.to_string()))?,
            None => File::default(),
        };
        let mut problems = Vec::new();
        // An empty variable counts as unset, as `HYPHA_MEDIA_URL=` did before.
        let env = |name: &str| env(name).filter(|v| !v.trim().is_empty());
        apply_env(&mut file, &env, &mut problems);
        let config = check(&file, &mut problems);
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
fn apply_env(file: &mut File, env: &impl Fn(&str) -> Option<String>, problems: &mut Vec<Problem>) {
    let flag = |name: &str, problems: &mut Vec<Problem>| match env(name).as_deref() {
        None => None,
        Some("true") => Some(true),
        Some("false") => Some(false),
        Some(other) => {
            problems.push(Problem {
                key: name.to_string(),
                message: format!("is true or false, not {other:?}"),
            });
            None
        }
    };
    if let Some(bind) = env("HYPHA_BIND") {
        file.server.bind = bind;
    } else if let Some(port) = env("PORT") {
        file.server.bind = format!("0.0.0.0:{port}");
    }
    if let Some(list) = env("HYPHA_TRUSTED_PROXIES") {
        file.server.trusted_proxies = list
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }
    if let Some(url) = env("HYPHA_ADMIN_URL") {
        file.server.admin_url = Some(url);
    }
    if let Some(allow) = flag("HYPHA_ALLOW_PRIVATE_CALLBACKS", problems) {
        file.outbound.allow_private = allow;
    }
    if let Some(url) = env("HYPHA_MEDIA_URL") {
        file.media.public_url = Some(url);
    }
    match env("HYPHA_MEDIA_TRANSFORMS").as_deref() {
        None => {}
        Some("none") => file.media.transforms = Transforms::None,
        Some("cloudflare") => file.media.transforms = Transforms::Cloudflare,
        Some(other) => problems.push(Problem {
            key: "HYPHA_MEDIA_TRANSFORMS".to_string(),
            message: format!("is `cloudflare` or `none`, not {other:?}"),
        }),
    }
    if let Some(endpoint) = env("HYPHA_S3_ENDPOINT") {
        file.media.s3.endpoint = Some(endpoint);
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
}

const MINUTE: Duration = Duration::from_secs(60);
const HOUR: Duration = Duration::from_secs(3600);
/// The longest an S3 presigned URL may work.
const PRESIGN_MAX: Duration = Duration::from_secs(7 * 24 * 3600);

/// Checks `file` (section 8) and converts it, adding a problem for each broken rule.
/// What it returns is only used when there are none.
fn check(file: &File, problems: &mut Vec<Problem>) -> Config {
    let mut problem = |key: &str, message: String| {
        problems.push(Problem {
            key: key.to_string(),
            message,
        })
    };
    let mut at_least = |key: &str, value: Dur, min: Duration| {
        if value.0 < min {
            problem(key, format!("is {value}, at least {} is needed", Dur(min)));
        }
    };

    // Server.
    let server = &file.server;
    let port_ok = server
        .bind
        .rsplit_once(':')
        .is_some_and(|(host, port)| !host.is_empty() && port.parse::<u16>().is_ok());
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
    let max_body = usize::try_from(server.max_body.0).unwrap_or(usize::MAX);
    let admin_url = server
        .admin_url
        .as_deref()
        .map(|url| url.trim_end_matches('/').to_string());

    // Database.
    at_least(
        "database.pool_timeout",
        file.database.pool_timeout,
        Duration::from_millis(1),
    );

    // Media.
    let media = &file.media;
    for (key, value) in [
        ("media.upload_expires", media.upload_expires),
        ("media.download_expires", media.download_expires),
    ] {
        at_least(key, value, MINUTE);
    }
    at_least(
        "media.abandoned_after",
        media.abandoned_after,
        media.upload_expires.0,
    );

    // Auth.
    let auth = &file.auth;
    at_least("auth.session_lifetime", auth.session_lifetime, MINUTE);
    at_least("auth.invite_lifetime", auth.invite_lifetime, MINUTE);
    for (key, value) in [
        ("auth.second_factor_window", auth.second_factor_window),
        ("auth.second_factor_lock", auth.second_factor_lock),
        ("auth.password_reset_lifetime", auth.password_reset_lifetime),
        (
            "auth.password_reset_resend_after",
            auth.password_reset_resend_after,
        ),
    ] {
        at_least(key, value, Duration::from_secs(1));
    }

    // Webhooks.
    let hooks = &file.webhooks;
    for (i, wait) in hooks.retry.iter().enumerate() {
        at_least(
            &format!("webhooks.retry[{i}]"),
            *wait,
            Duration::from_secs(1),
        );
    }
    at_least(
        "webhooks.retry_then_every",
        hooks.retry_then_every,
        Duration::from_secs(1),
    );
    let first_wait = hooks
        .retry
        .first()
        .copied()
        .unwrap_or(hooks.retry_then_every);
    at_least("webhooks.give_up_after", hooks.give_up_after, first_wait.0);

    // Jobs.
    let jobs = &file.jobs;
    for (key, value) in [
        ("jobs.dispatch.interval", jobs.dispatch.interval),
        ("jobs.prune.interval", jobs.prune.interval),
        ("jobs.media_sweep.interval", jobs.media_sweep.interval),
        ("jobs.prune.events_after", jobs.prune.events_after),
        ("jobs.prune.sessions_after", jobs.prune.sessions_after),
    ] {
        at_least(key, value, Duration::from_millis(1));
    }
    at_least(
        "jobs.prune.idempotency_keys_after",
        jobs.prune.idempotency_keys_after,
        HOUR,
    );
    // A lease must hold at least one delivery's longest run, with room to record it.
    at_least(
        "jobs.dispatch.lease",
        jobs.dispatch.lease,
        MINUTE.max(LONGEST_RUN),
    );

    // Rate limits.
    for (name, quota) in file.rate_limits.all() {
        at_least(
            &format!("rate_limits.{name}.per"),
            quota.per,
            Duration::from_millis(1),
        );
    }

    // The rules `at_least` can't express, after its last use.
    if !port_ok {
        problem(
            "server.bind",
            format!("{:?} isn't host:port, like \"0.0.0.0:3000\"", server.bind),
        );
    }
    if !bad_proxies.is_empty() {
        problem(
            "server.trusted_proxies",
            format!("not CIDR ranges or addresses: {bad_proxies:?}"),
        );
    }
    for (key, url) in [
        ("server.admin_url", &server.admin_url),
        ("media.public_url", &media.public_url),
        ("media.s3.endpoint", &media.s3.endpoint),
    ] {
        if let Some(url) = url
            && !(url.starts_with("https://") || url.starts_with("http://"))
        {
            problem(key, format!("{url:?} isn't an http:// or https:// URL"));
        }
    }
    if file.database.max_connections == Some(0) {
        problem("database.max_connections", "is at least 1".to_string());
    }
    for (key, value) in [
        ("media.upload_expires", media.upload_expires),
        ("media.download_expires", media.download_expires),
    ] {
        if value.0 > PRESIGN_MAX {
            problem(key, format!("is {value}, at most 7d (S3's limit)"));
        }
    }
    if media.s3.endpoint.is_some() && media.s3.bucket.is_none() {
        problem(
            "media.s3.bucket",
            "is required with an endpoint (or HYPHA_S3_BUCKET)".to_string(),
        );
    }
    if hooks.retry.len() > 20 {
        problem(
            "webhooks.retry",
            format!("has {} waits, at most 20", hooks.retry.len()),
        );
    }
    if !(1..=1_000).contains(&jobs.dispatch.batch) {
        problem(
            "jobs.dispatch.batch",
            format!("is {}, 1 to 1,000", jobs.dispatch.batch),
        );
    }
    for (name, quota) in file.rate_limits.all() {
        if quota.burst == 0 {
            problem(
                &format!("rate_limits.{name}.burst"),
                "is at least 1".to_string(),
            );
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
            Err(ConfigError::Invalid(problems)) => problems
                .into_iter()
                .map(|p| format!("{}: {}", p.key, p.message))
                .collect(),
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
        let Err(ConfigError::Parse(message)) = load("[jobs.dispatch]\nintervall = \"2s\"") else {
            panic!("a typo must fail");
        };
        assert!(message.contains("intervall"), "{message}");
        assert!(message.contains("line 2"), "{message}");
        let Err(ConfigError::Parse(message)) = load("[auth]\nsession_lifetime = \"7 days\"") else {
            panic!("a bad duration must fail");
        };
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
                "jobs.dispatch.batch: is 0, 1 to 1,000",
                "jobs.dispatch.lease: is 30s, at least 1m is needed",
                "jobs.prune.idempotency_keys_after: is 10m, at least 1h is needed",
                "media.abandoned_after: is 1h, at least 8d is needed",
                "media.s3.bucket: is required with an endpoint (or HYPHA_S3_BUCKET)",
                "media.s3.endpoint: \"minio:9000\" isn't an http:// or https:// URL",
                "media.upload_expires: is 8d, at most 7d (S3's limit)",
                "rate_limits.cart_create_ip.burst: is at least 1",
                "rate_limits.cart_create_ip.per: is 0s, at least 1ms is needed",
                "server.bind: \"3000\" isn't host:port, like \"0.0.0.0:3000\"",
                "server.trusted_proxies: not CIDR ranges or addresses: [\"nope\"]",
            ]
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
