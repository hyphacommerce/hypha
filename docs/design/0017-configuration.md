# 0017: Configuration

- **Status:** Accepted (2026-10-16). Danilo accepted the four recommendations in the
  discussion that led here: the four-way sort of values (CF1), a separate `hypha-config`
  crate (CF3), environment variables only for the settings that already have one (CF5),
  and this note before fulfilment (now 0018; search is 0019).
- **Scope:** where the values hard-coded today go (job timers, retention, lifetimes, rate
  limits, server limits), the `hypha.config.toml` file, how its settings reach domain
  crates without breaking the wasm rules, how it's typed for editors, and how it's
  checked at startup.
- **Not in scope:** merchant settings kept in the database and edited in the admin (the
  store's name today, fulfilment's settings next); the admin's own configuration (it stays
  in its Worker variables, section 7); hot reloading; secrets managers.
- **Related:** 0006 (one store per deployment; the store's name), 0008 (auth lifetimes),
  0009 (rate limits), 0013 (media uploads and the sweep), 0016 (jobs, retries,
  retention).

## Decisions

| | Decision |
| --- | --- |
| CF1 | **Four kinds of value, four homes.** Secrets and per-machine values stay in environment variables (`DATABASE_URL`, `HYPHA_SECRET_KEY`, the S3 keys, the owner's first login). Operator tuning goes in `hypha.config.toml`. Merchant settings live in the database and are edited in the admin. Contracts with clients stay constants (section 2) |
| CF2 | **Domain crates never read the file.** Each crate that has settings owns a plain struct with a `Default` holding today's values (`AuthSettings`, `DispatchSettings`, `MediaSettings`), passed in like a port: through `AuthEnv`, or as an argument. No globals, so tests pass `Default::default()` or a setting of their own |
| CF3 | **One crate turns the file into settings:** `hypha-config` holds the file's types, the defaults, the checks and the JSON Schema, and converts the file into each crate's settings. It parses a string and reads the environment through a function it's given, so it builds for wasm and a Workers host can use it. Only hosts depend on it |
| CF4 | **Defaults, then the file, then the environment.** No file: every default, as today. `HYPHA_CONFIG` names the file; without it, `hypha.config.toml` in the working directory is read if it's there. The settings that already have an environment variable keep it, and it wins over the file (section 5). *Hard to reverse* |
| CF5 | **No generic environment override per key** (`HYPHA__JOBS__DISPATCH__INTERVAL`) for now: every setting would have two names. A deployment that needs one sets the file, or we add the override later |
| CF6 | **Key names are a contract with operators,** like error codes: never renamed once released. Unknown keys fail startup (`deny_unknown_fields`), so a typo isn't silently ignored. *Hard to reverse* |
| CF7 | **Typed for editors by a generated JSON Schema,** `hypha.config.schema.json` at the repository root, from the Rust types (`schemars`, with their doc comments as descriptions). A file starting `#:schema ./hypha.config.schema.json` gets completion and checks in editors that read TOML schemas (Taplo, Even Better TOML). A test fails while the committed schema is stale, like the OpenAPI documents |
| CF8 | **Durations and sizes are written for people:** `"2s"`, `"30d"`, `"2MiB"`, not counts of seconds or bytes |
| CF9 | **Checked at startup, all at once.** A bad value stops the host with every problem and its key (`jobs.dispatch.lease: at least 1m …`). Rules that span settings are checked too: a dispatch lease must outlast the slowest webhook |
| CF10 | **Read once, at startup.** A change needs a restart. `hypha-host-axum config` prints the settings in effect (defaults, file and environment together) as TOML |

## 1. What others do

- **Medusa** has `medusa-config.ts`, a typed TypeScript file the server imports, with
  secrets read from the environment inside it.
- **Saleor** and **Django** apps read settings from a Python module and the environment.
- **Rust services** usually deserialize a TOML file into structs with serde, sometimes
  layered with the environment through `figment` or `config`.
- **Cloudflare Workers** have no filesystem: settings are `wrangler.toml` variables, and
  timers are Cron triggers there.

Hypha takes the Rust convention (TOML into typed structs), keeps Medusa's line between
secrets in the environment and settings in a file, and keeps the parsing free of the
filesystem for Workers.

## 2. What stays a constant (CF1)

These are contracts with clients or other software, not tuning:

- event types and versions, error codes;
- TOTP's 6 digits and 30-second step (authenticator apps assume them), token and code
  lengths;
- `Money::MAX_AMOUNT`, `PageLimit::DEFAULT` and `MAX` (in the OpenAPI documents);
- the bounds on webhook and rate-app timeouts, the limits on variants and media per
  product, the size limits per kind of file (the admin's messages quote them; making them
  settings means sending them in errors' `details` first);
- the counts of wrong second-factor codes before a lock or a revoked session (5);
- the retries of a command that lost a race (`hypha_core::MAX_ATTEMPTS`).

## 3. The file

Every key, at its default (commented out: unset by default). Every key is optional.

```toml
#:schema ./hypha.config.schema.json

[server]
bind = "127.0.0.1:3000"     # HYPHA_BIND, then PORT ("0.0.0.0:$PORT"), win
trusted_proxies = []        # CIDR ranges; HYPHA_TRUSTED_PROXIES wins
# admin_url = "https://admin.example.com"  # for links in emails; HYPHA_ADMIN_URL wins
max_body = "2MiB"
shutdown_grace = "10s"      # how long stopping waits for jobs and background work

[database]
pool_timeout = "5s"         # waiting for, opening or recycling a connection
# max_connections = 16      # unset: the pool's default (4 per CPU)

[outbound]
allow_private = false       # HYPHA_ALLOW_PRIVATE_CALLBACKS wins; development only

[media]
# public_url = "https://cdn.example.com"  # HYPHA_MEDIA_URL wins; unset: signed URLs
transforms = "none"         # or "cloudflare"; HYPHA_MEDIA_TRANSFORMS wins
upload_expires = "15m"
download_expires = "15m"   # a private file's signed URL
abandoned_after = "1d"

[media.s3]                  # no endpoint: media is off. The keys stay in the environment.
# endpoint = "https://…"    # HYPHA_S3_ENDPOINT wins
region = "auto"             # HYPHA_S3_REGION wins
# bucket = "hypha"         # HYPHA_S3_BUCKET wins; required with an endpoint
create_bucket = false       # HYPHA_S3_CREATE_BUCKET wins; development only

[auth]
session_lifetime = "7d"
second_factor_window = "5m" # after the password, to enter the code
second_factor_lock = "15m"  # after 5 wrong codes in a row
password_reset_lifetime = "1h"
password_reset_resend_after = "5m"
invite_lifetime = "7d"

[webhooks]
retry = ["1m", "5m", "30m", "2h", "6h"]
retry_then_every = "12h"
give_up_after = "3d"

[jobs]
enabled = true              # false: this instance runs none (section 6)

[jobs.dispatch]
enabled = true
interval = "2s"
batch = 100
lease = "2m"

[jobs.prune]
enabled = true
interval = "1h"
events_after = "30d"
idempotency_keys_after = "1d"
sessions_after = "30d"      # after they ended

[jobs.media_sweep]
enabled = true
interval = "1h"

[rate_limits]               # each: up to `burst` at once, one more per `per`
login_ip = { burst = 10, per = "30s" }
login_email_from_ip = { burst = 5, per = "1m" }
login_email = { burst = 50, per = "72s" }
mfa_ip = { burst = 10, per = "30s" }
reset_ip = { burst = 5, per = "1m" }
reset_email_from_ip = { burst = 3, per = "5m" }
reset_confirm_ip = { burst = 10, per = "30s" }
invite_ip = { burst = 10, per = "30s" }
shipping_quotes_cart = { burst = 10, per = "2s" }
cart_create_ip = { burst = 20, per = "3s" }
```

The rate-limit keys are `Rule`'s variants in snake case: still a closed list, now with
their quotas outside the code. The idle rate-limit keys are forgotten every minute; that's
housekeeping, not a setting.

## 4. How settings reach the code (CF2, CF3)

```text
hypha.config.toml ─┐
environment ───────┼─► hypha-config: Config::load(text, env) ─► Config (checked)
defaults ──────────┘           │
                               ├─► AuthSettings      (hypha-auth)      → HostEnv: AuthEnv
                               ├─► DispatchSettings  (hypha-dispatch)  → job arguments
                               ├─► MediaSettings     (hypha-media)     → command arguments
                               └─► server, database, outbound, media URLs and S3,
                                   rate limits, job timers               → the host
```

- **Domain settings are plain structs** with `std::time::Duration` fields (just a number;
  it builds for wasm, unlike `Instant` and `SystemTime`) and a `Default` with today's
  constants, which go away. `Timestamp` gains `plus`/`minus` taking a `Duration`.
- **Auth** reads its lifetimes from `AuthEnv`, which every auth and staff command already
  takes: the trait gains `HasAuthSettings` (`fn auth_settings(&self) -> &AuthSettings`).
  `HostEnv` holds them; testkit's `TestEnv` holds the defaults and can change them.
  Aggregates take the lifetime they need (`Session::active(…, lifetime)`), so they stay
  free of settings. The invite's lifetime is here too: an invite is a token
  `hypha-auth` issues.
- **Dispatch and media** take theirs as an argument (`fan_out(&ports, &clock, &settings)`,
  `create_upload(…, &settings, input)`), like the clock. The retry schedule becomes
  `RetrySchedule::next_attempt`, and the attempts a delivery gets follow from it.
- **`hypha-config` converts and checks.** Its types derive `Deserialize`, `Serialize` and
  `JsonSchema` and are plain data; the domain structs derive none of them, so CLAUDE.md's
  rule on `Deserialize` holds. `Config::load(text: Option<&str>, env: impl Fn(&str) ->
  Option<String>)` takes the environment as a function: the native host passes
  `std::env::var`, a Workers host its `env.var`, tests a map.

The crate sits in a new layer:

| Layer | Crates | May depend on |
| --- | --- | --- |
| Hosts | `hypha-host-axum` | anything |
| Configuration | `hypha-config` | workflows, domain crates, core; only hosts depend on it |

## 5. The environment (CF4, CF5)

These variables keep working and win over the file:

| Variable | Key |
| --- | --- |
| `HYPHA_BIND`, then `PORT` | `server.bind` |
| `HYPHA_TRUSTED_PROXIES` | `server.trusted_proxies` (comma-separated) |
| `HYPHA_ADMIN_URL` | `server.admin_url` |
| `HYPHA_ALLOW_PRIVATE_CALLBACKS` | `outbound.allow_private` (`true`) |
| `HYPHA_MEDIA_URL`, `HYPHA_MEDIA_TRANSFORMS` | `media.public_url`, `media.transforms` |
| `HYPHA_S3_ENDPOINT`, `_REGION`, `_BUCKET`, `_CREATE_BUCKET` | `media.s3.*` |

Only in the environment, never in the file: `DATABASE_URL`, `HYPHA_SECRET_KEY`,
`HYPHA_S3_ACCESS_KEY_ID`, `HYPHA_S3_SECRET_ACCESS_KEY`, `HYPHA_OWNER_EMAIL`,
`HYPHA_OWNER_PASSWORD`, `HYPHA_OWNER_NAME`, `HYPHA_STORE_NAME` (the last four only matter
on the first start, and the store's name moves to the admin with the store settings),
`HYPHA_CONFIG` and `RUST_LOG`.

A file that names a secret is refused: there's no key for one, so it's an unknown key.

## 6. Jobs on more than one instance

`jobs.enabled = false` runs a web-only instance; one other instance, with jobs on, does
the background work. Nothing depends on it for correctness (deliveries are leased, and
pruning twice is harmless), but it keeps the load off the instances serving requests.
A Workers host schedules jobs with Cron triggers in `wrangler.toml` and ignores the
intervals; the schema says so.

## 7. The admin

The admin is its own deployment and doesn't read `hypha.config.toml`. Its few settings
(`HYPHA_API_URL`, `ADMIN_TRUST_X_FORWARDED_FOR`) stay Worker variables. Nothing it shows
depends on a setting yet: the limits its messages quote are the constants of section 2.
When one does, it comes from the API (in the error's message or `details`, or a
read-only settings route), never from a copy in the admin.

## 8. Checks (CF9)

- Every duration and interval is above zero; `burst` is at least 1.
- `jobs.dispatch.batch` is 1 to 1,000.
- `jobs.dispatch.lease` is at least the slowest webhook's run (its 30-second timeout plus
  15 seconds) plus 15 seconds, so at least one delivery fits in a lease: 1 minute.
- `webhooks.retry` holds at most 20 waits; `give_up_after` is at least the first wait.
- `jobs.prune.idempotency_keys_after` is at least an hour, so a client retrying a slow
  request still gets its first answer.
- `auth.session_lifetime` and `auth.invite_lifetime` are at least a minute.
- `media.upload_expires` and `download_expires` are 1 minute to 7 days (S3's limit on
  signed URLs); `media.abandoned_after` is at least `upload_expires`, so an upload in
  progress isn't swept; `media.s3.bucket` is required with an endpoint.
- `server.bind` is `host:port`, `trusted_proxies` are CIDR ranges or addresses,
  `admin_url`, `media.public_url` and `media.s3.endpoint` are `http(s)://` URLs,
  `media.transforms` is `none` or `cloudflare`, and the flags in the environment are
  `true` or `false`. An empty environment variable counts as unset.

A file that fails to parse names the line and column; one that parses but fails a check
lists every failure, each with its key, sorted by key.

## 9. Order of work

Built in the other order (section 10): the domain settings first, then these.

1. **`hypha-config` and the host.** The crate: file types, defaults, the environment, the
   checks, the schema (with its staleness test) and the `config` command. The host loads
   it and stops reading the environment itself for anything but secrets and first-start
   values: server, database, outbound, media URLs and S3, rate limits, job timers and
   switches.
2. **Domain settings.** `DispatchSettings` (batch, lease, retry schedule, retention),
   `MediaSettings` (upload expiry, abandoned uploads), `AuthSettings` (session, second
   factor, reset, invite lifetimes); their constants become the defaults.
3. **Docs.** CLAUDE.md (the layer, the commands, the file), the wasm build list, and this
   note's as-built section.

## 10. As built (2026-10-16)

Built on `feat/config` in two commits, the domain settings first (a refactor: every
default is the old constant) and then `hypha-config` with the host.

- **Domain settings.** `AuthSettings` (`hypha-auth`, read through `AuthEnv`'s new
  `HasAuthSettings`; testkit's `TestEnv` has a public `auth` field), `DispatchSettings`
  with a `RetrySchedule` (`hypha-dispatch`; `MAX_ATTEMPTS` became
  `RetrySchedule::max_attempts`, `BATCH` a field, `LONGEST_RUN` a public `Duration` the
  checks use), `MediaSettings` (`hypha-media`, with `download_expires` too). The
  aggregates take the one lifetime they need, and `Timestamp` gained `plus` and `minus`
  for `Duration`s.
- **`hypha-config`.** `file.rs` holds the TOML types, whose `Default`s come from the
  domain settings; `units.rs` the `"30s"` and `"2MiB"` types (one number and one unit;
  durations print in the largest exact unit). `Config::load(text, env)` applies the
  environment, checks and converts; `Config::to_toml` prints what's in effect;
  `schema_json` is the schema (`schemars`), committed as `hypha.config.schema.json` and
  checked by a test.
- **The host** reads the file (`load_config` in `main.rs`) and keeps the `Config` in
  `AppState`; the body limit (`JsonBody`), the pool, the rate limiter, the trusted
  proxies, media, the outbound policy, the job runner and the shutdown grace read it.
  The rate limiter's housekeeping (forgetting idle keys) runs even with jobs off.
  `hypha-host-axum config` and `config schema` are new commands.
- **Not done:** a read-only settings route for the admin (section 7), an example file
  beyond section 3, and environment overrides for every key (CF5).
