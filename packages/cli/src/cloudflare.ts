// `--target cloudflare` (design note 0026, §5): the API as a Worker over Hyperdrive, with
// R2 for files, KV for the jobs' marks (W7) and a Rate Limiting binding per rule (W8).
// Any Postgres works (Neon, Supabase, RDS, your own): nothing here knows a provider. The
// Worker is placed next to the database by measuring its host (W12), not by a region
// read off a provider's hostname.
//
// `hypha deploy` does, each time (each step does nothing that's already done):
//
// 1. `hypha-host-axum setup` against the database's direct URL (W9);
// 2. creates what's missing: Hyperdrive (query cache off, always), the KV namespace, the
//    R2 bucket with its CORS rule and custom domain;
// 3. writes `wrangler.toml` from `hypha.json` and deploys the Worker (`worker-build`),
//    with its secrets;
// 4. marks the jobs as waiting (setup may have queued a search rebuild) and waits for
//    `GET /health`.

import { readFile, rm, writeFile } from "node:fs/promises";
import { join, relative } from "node:path";

import * as p from "@clack/prompts";

import { waitForHealth } from "./api.ts";
import { type Deployment, type EnvFile, saveDeployment, url } from "./deployment.ts";
import { run } from "./shell.ts";
import { REPO } from "./templates.ts";

/** The generated configuration, next to `hypha.json`. */
export const WRANGLER_FILE = "wrangler.toml";
const SECRETS_FILE = ".secrets.json";
const WORKER_CRATE = join(REPO, "crates", "hypha-host-workers");

/** The window of every Rate Limiting binding, in seconds (the longer of the two offered). */
export const WINDOW_SECONDS = 60;

/** A rule's quota in effect, as `hypha-host-axum config rate-limits` prints it. */
export type Quota = { burst: number; per_ms: number };

/** The binding for the rule under `[rate_limits]` key `key` (the Worker's `limiter::binding`). */
export function bindingName(key: string): string {
  return `RL_${key.toUpperCase()}`;
}

/** The most a quota allows in one window: its burst, plus one per period that fits (the
 * Worker's `limiter::binding_limit`). The binding then never refuses what the quota
 * allows, and caps every isolate together. */
export function bindingLimit(quota: Quota): number {
  return quota.burst + Math.floor((WINDOW_SECONDS * 1000) / Math.max(quota.per_ms, 1));
}

/** The database's `host:port`, which the Worker is placed next to. */
export function databaseHost(databaseUrl: string): string {
  const parsed = new URL(databaseUrl);
  return `${parsed.hostname.replace(/^\[|\]$/g, "")}:${parsed.port || "5432"}`;
}

/** Why `databaseUrl` can't be a Worker's database, if it can't. */
export function checkDatabaseUrl(databaseUrl: string | undefined): string | undefined {
  let parsed: URL;
  try {
    parsed = new URL(databaseUrl ?? "");
  } catch {
    return "a Postgres URL like postgres://user:password@host:5432/database";
  }
  if (parsed.protocol !== "postgres:" && parsed.protocol !== "postgresql:") return "a postgres:// URL";
  if (!parsed.username || !parsed.password) return "a URL with the user and password";
  if (/^(localhost|127\.|\[?::1\]?$|10\.|192\.168\.|172\.(1[6-9]|2\d|3[01])\.)/.test(parsed.hostname)) {
    return "a database Cloudflare can reach: Hyperdrive connects from Cloudflare's network, not from here";
  }
  return undefined;
}

/** The R2 bucket's S3 endpoint. */
export function r2Endpoint(accountId: string): string {
  return `https://${accountId}.r2.cloudflarestorage.com`;
}

export type WorkerInput = {
  /** This deployment's folder: paths in the file are relative to it. */
  dir: string;
  accountId: string;
  databaseUrl: string;
  hyperdriveId: string;
  kvId: string;
  /** `hypha.config.toml`'s text, the Worker's `HYPHA_CONFIG`. */
  config: string;
  quotas: Record<string, Quota>;
  compatibilityDate: string;
  /** With R2 keys; without, media is off. */
  media: boolean;
};

/** A TOML string: JSON's escapes are TOML's. */
const str = (value: string) => JSON.stringify(value);

/** The Worker's `wrangler.toml`, written on every deploy from `hypha.json`. */
export function wranglerToml(d: Deployment, input: WorkerInput): string {
  const cf = d.cloudflare!;
  const crate = relative(input.dir, WORKER_CRATE) || ".";
  const vars: [string, string][] = [["HYPHA_CONFIG", input.config]];
  if (input.media) {
    vars.push(
      ["HYPHA_S3_ENDPOINT", r2Endpoint(input.accountId)],
      ["HYPHA_S3_REGION", "auto"],
      ["HYPHA_S3_BUCKET", bucket(d)],
    );
    // Public files from the bucket's own domain; without one, signed URLs.
    if (cf.zoneId) vars.push(["HYPHA_MEDIA_URL", url(d, d.domains.media)]);
  }
  if (d.domains.admin) vars.push(["HYPHA_ADMIN_URL", url(d, d.domains.admin)]);
  const placement = cf.region
    ? ["[placement]", 'mode = "targeted"', `region = ${str(cf.region)}`]
    : ["[placement]", 'mode = "targeted"', `host = ${str(databaseHost(input.databaseUrl))}`];
  const limits = Object.entries(input.quotas)
    .sort(([a], [b]) => a.localeCompare(b))
    .flatMap(([key, quota], i) => [
      "[[ratelimits]]",
      `name = ${str(bindingName(key))}`,
      `namespace_id = ${str(String(cf.rateLimitBase + i))}`,
      `simple = { limit = ${bindingLimit(quota)}, period = ${WINDOW_SECONDS} }`,
      "",
    ]);
  return [
    "# Written by `hypha deploy` from hypha.json and hypha.config.toml on every deploy",
    "# (design note 0026): change those, not this file.",
    `name = ${str(workerName(d))}`,
    `main = ${str(`${crate}/build/worker/shim.mjs`)}`,
    `compatibility_date = ${str(input.compatibilityDate)}`,
    `account_id = ${str(input.accountId)}`,
    "workers_dev = false",
    `routes = [{ pattern = ${str(d.domains.api)}, custom_domain = true }]`,
    "",
    "[build]",
    'command = "worker-build --release"',
    `cwd = ${str(crate)}`,
    "",
    "# Next to the database: its round trips are most of a request (W12).",
    ...placement,
    "",
    "# Query cache off: the API's reads outside transactions (sessions, keys) must be fresh.",
    "[[hyperdrive]]",
    'binding = "HYPERDRIVE"',
    `id = ${str(input.hyperdriveId)}`,
    "",
    "[[kv_namespaces]]",
    'binding = "JOBS"',
    `id = ${str(input.kvId)}`,
    "",
    "# Per rule: the most its [rate_limits] quota allows in a minute (W8).",
    ...limits,
    "[triggers]",
    'crons = ["* * * * *"]',
    "",
    "[observability]",
    "enabled = true",
    "",
    "[vars]",
    ...vars.map(([key, value]) => `${key} = ${str(value)}`),
    "",
  ].join("\n");
}

export const workerName = (d: Deployment) => `${d.cloudflare!.name}-api`;
export const bucket = (d: Deployment) => `${d.cloudflare!.name}-media`;
const kvTitle = (d: Deployment) => `${d.cloudflare!.name}-jobs`;
const hyperdriveName = (d: Deployment) => d.cloudflare!.name;

/** The first 32-hex-digit ID in wrangler's output (what `create` commands print). */
export function idIn(output: string): string | undefined {
  return /\b[0-9a-f]{32}\b/.exec(output)?.[0];
}

/** R2's CORS rule: the admin uploads files straight to the bucket with signed URLs. */
export function corsRules(adminOrigin: string): string {
  return `${JSON.stringify(
    {
      rules: [
        {
          allowed: { origins: [adminOrigin], methods: ["GET", "HEAD", "PUT"], headers: ["content-type"] },
          exposeHeaders: ["ETag"],
          maxAgeSeconds: 3600,
        },
      ],
    },
    null,
    2,
  )}\n`;
}

/** What the deploy runs: here, or recorded in tests. */
export interface Tools {
  /** `wrangler …` with the account set; `capture` returns its output. */
  wrangler(args: string[], options?: { capture?: boolean }): Promise<string>;
  /** Builds the native host and returns its binary's path. */
  buildHost(): Promise<string>;
  /** Runs the native host's binary with only `env` (plus `PATH`/`HOME`), capturing. */
  host(binary: string, args: string[], env: Record<string, string>): Promise<string>;
  /** Fails with what to install when the Rust toolchain for Workers is missing. */
  checkToolchain(): Promise<void>;
  /** Milliseconds since the epoch. */
  now(): number;
  /** Whether the deployed API answers `GET /health` within a few minutes. */
  answers(d: Deployment): Promise<boolean>;
}

/** Hyperdrive's origin flags: the URL without its query (TLS is `--sslmode`, which
 * Hyperdrive takes for `require` and stricter), and the query cache always off. */
export function hyperdriveOrigin(databaseUrl: string): string[] {
  const parsed = new URL(databaseUrl);
  const sslmode = parsed.searchParams.get("sslmode");
  parsed.search = "";
  return [
    "--connection-string",
    parsed.toString(),
    ...(sslmode && ["require", "verify-ca", "verify-full"].includes(sslmode) ? ["--sslmode", sslmode] : []),
    "--caching-disabled",
  ];
}

/** `work`, with the database password left out of its error (the command line). */
async function redacted<T>(work: Promise<T>, password: string): Promise<T> {
  try {
    return await work;
  } catch (e) {
    const message = e instanceof Error ? e.message : String(e);
    throw new Error(password ? message.replaceAll(password, "***").replaceAll(encodeURIComponent(password), "***") : message);
  }
}

/** What's already there is fine: wrangler says so in its error. */
async function tolerate(work: Promise<unknown>, already: RegExp): Promise<void> {
  try {
    await work;
  } catch (e) {
    if (!already.test(e instanceof Error ? e.message : String(e))) throw e;
  }
}

export async function deployCloudflare(dir: string, d: Deployment, env: EnvFile, tools: Tools): Promise<void> {
  const cf = d.cloudflare;
  if (!cf) throw new Error("hypha.json: a cloudflare deployment needs `cloudflare`");
  const databaseUrl = env.get("DATABASE_URL");
  const problem = checkDatabaseUrl(databaseUrl);
  if (!databaseUrl || problem) throw new Error(`.env: DATABASE_URL should be ${problem}`);
  const secretKey = env.get("HYPHA_SECRET_KEY");
  if (!secretKey) throw new Error(".env: HYPHA_SECRET_KEY is missing");
  const keyId = env.get("R2_ACCESS_KEY_ID");
  const keySecret = env.get("R2_SECRET_ACCESS_KEY");
  const media = Boolean(keyId && keySecret);

  // Who's logged in, and to which account.
  const who = JSON.parse(await tools.wrangler(["whoami", "--json"], { capture: true }).catch(() => {
    throw new Error("wrangler isn't logged in: run `wrangler login` (or set CLOUDFLARE_API_TOKEN)");
  })) as { accounts?: { id: string; name: string }[] };
  if (!cf.accountId) {
    const accounts = who.accounts ?? [];
    if (accounts.length !== 1) {
      throw new Error(
        `wrangler can reach ${accounts.length} accounts: pick one with \`hypha init --force --account-id …\`` +
          (accounts.length ? `\n${accounts.map((a) => `${a.id}  ${a.name}`).join("\n")}` : ""),
      );
    }
    cf.accountId = accounts[0].id;
    await saveDeployment(dir, d);
  }
  const accountId = cf.accountId;
  await tools.checkToolchain();

  // A spinner left turning on a failure would keep the process alive.
  const spin = p.spinner();
  let spinning = false;
  const start = (message: string) => {
    spin.start(message);
    spinning = true;
  };
  const stop = (message: string, failed = false) => {
    if (failed) spin.error(message);
    else spin.stop(message);
    spinning = false;
  };
  try {
    // 1. The database (W9): migrations, the store, the owner, the location, search.
    start("Building the setup tool (hypha-host-axum)");
    const binary = await tools.buildHost();
    const hostEnv = {
      DATABASE_URL: databaseUrl,
      HYPHA_SECRET_KEY: secretKey,
      HYPHA_CONFIG: join(dir, "hypha.config.toml"),
      ...pick(env, ["HYPHA_OWNER_EMAIL", "HYPHA_OWNER_PASSWORD", "HYPHA_OWNER_NAME", "HYPHA_STORE_NAME"]),
    };
    spin.message("Setting the database up");
    await tools.host(binary, ["setup"], hostEnv);
    const quotas = JSON.parse(await tools.host(binary, ["config", "rate-limits"], hostEnv)) as Record<string, Quota>;
    stop("Database ready");

    // 2. What the Worker binds to.
    start("Creating Hyperdrive, KV and R2 where missing");
    const origin = hyperdriveOrigin(databaseUrl);
    const password = decodeURIComponent(new URL(databaseUrl).password);
    if (cf.hyperdriveId) {
      // Every deploy: the URL may have changed, and the cache must stay off.
      await redacted(tools.wrangler(["hyperdrive", "update", cf.hyperdriveId, ...origin], { capture: true }), password);
    } else {
      const out = await redacted(
        tools.wrangler(["hyperdrive", "create", hyperdriveName(d), ...origin], { capture: true }),
        password,
      );
      cf.hyperdriveId = idIn(out);
      if (!cf.hyperdriveId) throw new Error(`no Hyperdrive ID in wrangler's answer:\n${out}`);
      await saveDeployment(dir, d);
    }
    if (!cf.kvId) {
      const listed = JSON.parse(await tools.wrangler(["kv", "namespace", "list"], { capture: true })) as {
        id: string;
        title: string;
      }[];
      cf.kvId =
        listed.find((n) => n.title === kvTitle(d))?.id ??
        idIn(await tools.wrangler(["kv", "namespace", "create", kvTitle(d)], { capture: true }));
      if (!cf.kvId) throw new Error("no KV namespace ID in wrangler's answer");
      await saveDeployment(dir, d);
    }
    if (media) {
      await tolerate(tools.wrangler(["r2", "bucket", "create", bucket(d)], { capture: true }), /already exists|10004/i);
      if (d.domains.admin) {
        const rules = join(dir, ".cors.json");
        await writeFile(rules, corsRules(url(d, d.domains.admin)));
        try {
          await tools.wrangler(["r2", "bucket", "cors", "set", bucket(d), "--file", rules, "--force"], { capture: true });
        } finally {
          await rm(rules, { force: true });
        }
      }
      if (cf.zoneId) {
        await tolerate(
          tools.wrangler(
            ["r2", "bucket", "domain", "add", bucket(d), "--domain", d.domains.media, "--zone-id", cf.zoneId, "--force"],
            { capture: true },
          ),
          /already|exists/i,
        );
      }
    }
    stop("Resources ready");

    // 3. The Worker, with its secrets.
    const compatibilityDate =
      /^compatibility_date\s*=\s*"([^"]+)"/m.exec(await readFile(join(WORKER_CRATE, WRANGLER_FILE), "utf8"))?.[1] ??
      "2026-10-01";
    await writeFile(
      join(dir, WRANGLER_FILE),
      wranglerToml(d, {
        dir,
        accountId,
        databaseUrl,
        hyperdriveId: cf.hyperdriveId,
        kvId: cf.kvId,
        config: await readFile(join(dir, "hypha.config.toml"), "utf8"),
        quotas,
        compatibilityDate,
        media,
      }),
    );
    const secrets: Record<string, string> = { HYPHA_SECRET_KEY: secretKey };
    if (media) Object.assign(secrets, { HYPHA_S3_ACCESS_KEY_ID: keyId!, HYPHA_S3_SECRET_ACCESS_KEY: keySecret! });
    const secretsPath = join(dir, SECRETS_FILE);
    await writeFile(secretsPath, JSON.stringify(secrets), { mode: 0o600 });
    p.log.step("Building and deploying the Worker (worker-build --release)…");
    try {
      await tools.wrangler(["deploy", "--config", join(dir, WRANGLER_FILE), "--secrets-file", secretsPath]);
    } finally {
      await rm(secretsPath, { force: true });
    }

    // 4. Setup may have queued work (a search rebuild): the next tick runs the jobs (W7).
    await tools.wrangler(
      ["kv", "key", "put", "jobs:written", String(tools.now()), "--namespace-id", cf.kvId, "--remote"],
      { capture: true },
    );
    start("Waiting for the API (a new custom domain can take a minute)");
    if (!(await tools.answers(d))) {
      stop("The API didn't answer", true);
      throw new Error(`${url(d, d.domains.api)}/health didn't answer in 5 minutes: see \`hypha logs\``);
    }
    stop("Running");
    if (!media) p.log.warn("Media is off: add an R2 API token's keys to .env (R2_ACCESS_KEY_ID, R2_SECRET_ACCESS_KEY), then deploy.");
    else if (!cf.zoneId) p.log.warn("Files are served by signed URLs: pass --zone-id to `hypha init --force` for the media domain.");
  } catch (e) {
    if (spinning) stop("Stopped", true);
    throw e;
  }
}

function pick(env: EnvFile, keys: string[]): Record<string, string> {
  const out: Record<string, string> = {};
  for (const key of keys) {
    const value = env.get(key);
    if (value) out[key] = value;
  }
  return out;
}

/** The tools on this machine: wrangler (logged in, or `CLOUDFLARE_API_TOKEN`), cargo,
 * and `worker-build` with the wasm target. */
export function hereTools(d: Deployment): Tools {
  return {
    wrangler(args, { capture } = {}) {
      const accountId = d.cloudflare?.accountId;
      return run("wrangler", args, { capture, cwd: REPO, env: accountId ? { CLOUDFLARE_ACCOUNT_ID: accountId } : {} });
    },
    async buildHost() {
      await run("cargo", ["build", "--release", "-p", "hypha-host-axum"], { cwd: REPO, capture: true });
      return join(process.env.CARGO_TARGET_DIR ?? join(REPO, "target"), "release", "hypha-host-axum");
    },
    // Only what setup needs: no DATABASE_URL, HYPHA_* or S3 settings from this shell.
    host: (binary, args, env) => run(binary, args, { cwd: REPO, env: { RUST_LOG: "info", ...env }, bareEnv: true, capture: true }),
    async checkToolchain() {
      await run("worker-build", ["--version"], { capture: true }).catch(() => {
        throw new Error("worker-build is missing: `cargo install worker-build`");
      });
      const targets = await run("rustup", ["target", "list", "--installed"], { capture: true }).catch(() => "");
      if (!targets.includes("wasm32-unknown-unknown")) {
        throw new Error("the wasm target is missing: `rustup target add wasm32-unknown-unknown`");
      }
    },
    now: () => Date.now(),
    answers: (d) => waitForHealth(d, 300),
  };
}
