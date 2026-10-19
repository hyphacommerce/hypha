import assert from "node:assert/strict";
import { mkdtemp, readdir, readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import {
  type Binding,
  checkDatabaseUrl,
  databaseHost,
  deployCloudflare,
  hyperdriveOrigin,
  type Tools,
  wranglerToml,
} from "./cloudflare.ts";
import { profileOf } from "./apps.ts";
import { MEILI_TEMPLATE } from "./fixtures.ts";
import { type Deployment, EnvFile, loadDeployment, saveDeployment } from "./deployment.ts";
import { systemEnv } from "./shell.ts";
import { REPO } from "./templates.ts";

const D1_ID = "0f2c7a1e-5b3d-4c8e-9a6f-1d2e3f405162";
const REV = "a".repeat(40);

const DB = "postgres://hypha:secret@ep-quiet-sky.eu-central-1.aws.neon.tech/hypha?sslmode=require";
const PLAIN = "postgres://hypha:secret@ep-quiet-sky.eu-central-1.aws.neon.tech/hypha";

function deployment(): Deployment {
  return {
    version: 1,
    target: "cloudflare",
    remoteDir: "",
    buildOn: "here",
    scheme: "https",
    domains: { api: "api.example.com", media: "media.example.com", admin: "admin.example.com" },
    cloudflare: { name: "hypha", rateLimitBase: 5000 },
  };
}

/** As `hypha-host-axum config rate-limits` prints them (a few). */
const BINDINGS: Binding[] = [
  { rule: "login_ip", binding: "RL_LOGIN_IP", slot: 0, limit: 12, period: 60 },
  { rule: "login_email", binding: "RL_LOGIN_EMAIL", slot: 2, limit: 50, period: 60 },
  {
    rule: "shipping_quotes_cart",
    binding: "RL_SHIPPING_QUOTES_CART",
    slot: 8,
    limit: 40,
    period: 60,
  },
];

test("the Worker is placed next to the database's host, whoever runs it", () => {
  assert.equal(databaseHost(DB), "ep-quiet-sky.eu-central-1.aws.neon.tech:5432");
  assert.equal(databaseHost("postgres://u:p@db.example.com:6543/x"), "db.example.com:6543");
  assert.equal(databaseHost("postgres://u:p@[2001:db8::1]:5432/x"), "[2001:db8::1]:5432");
});

test("a database Cloudflare can't reach, or without credentials, is refused", () => {
  assert.equal(checkDatabaseUrl(DB), undefined);
  assert.match(checkDatabaseUrl("postgres://u:p@localhost:5432/x") ?? "", /Cloudflare can reach/);
  assert.match(checkDatabaseUrl("postgres://u:p@192.168.1.4/x") ?? "", /Cloudflare can reach/);
  assert.match(checkDatabaseUrl("postgres://db.example.com/x") ?? "", /user and password/);
  assert.match(checkDatabaseUrl("mysql://u:p@db.example.com/x") ?? "", /postgres/);
  assert.match(checkDatabaseUrl("not a url") ?? "", /Postgres URL/);
});

function toml(d: Deployment, media = true) {
  return wranglerToml(d, {
    dir: join(REPO, "hypha-deploy"),
    accountId: "a".repeat(32),
    databaseUrl: DB,
    hyperdriveId: "h".repeat(32),
    kvId: "k".repeat(32),
    config: '[rate_limits.login_ip]\nburst = 20\nper = "30s"\n',
    bindings: BINDINGS,
    compatibilityDate: "2026-10-01",
    media,
  });
}

test("wrangler.toml binds Hyperdrive, KV and a rate limiter per rule, and is placed by the database", () => {
  const file = toml(deployment());
  assert.match(file, /^name = "hypha-api"$/m);
  assert.match(file, /^routes = \[\{ pattern = "api\.example\.com", custom_domain = true \}\]$/m);
  assert.match(
    file,
    /\[placement\]\nmode = "targeted"\nhost = "ep-quiet-sky\.eu-central-1\.aws\.neon\.tech:5432"/,
  );
  assert.match(file, /\[\[hyperdrive\]\]\nbinding = "HYPERDRIVE"\nid = "h{32}"/);
  assert.match(file, /\[\[kv_namespaces\]\]\nbinding = "JOBS"\nid = "k{32}"/);
  // Each rule at its own slot in the deployment's block, whatever else is listed.
  assert.match(
    file,
    /name = "RL_LOGIN_IP"\nnamespace_id = "5000"\nsimple = \{ limit = 12, period = 60 \}/,
  );
  assert.match(
    file,
    /name = "RL_LOGIN_EMAIL"\nnamespace_id = "5002"\nsimple = \{ limit = 50, period = 60 \}/,
  );
  assert.match(
    file,
    /name = "RL_SHIPPING_QUOTES_CART"\nnamespace_id = "5008"\nsimple = \{ limit = 40, period = 60 \}/,
  );
  assert.match(
    file,
    /^HYPHA_CONFIG = "\[rate_limits\.login_ip\]\\nburst = 20\\nper = \\"30s\\"\\n"$/m,
  );
  assert.match(file, /^HYPHA_ADMIN_URL = "https:\/\/admin\.example\.com"$/m);
  assert.match(file, /^main = "\.\.\/crates\/hypha-host-workers\/build\/worker\/shim\.mjs"$/m);
  // Wherever wrangler runs from: it reads the build's cwd as given.
  assert.ok(file.includes(`cwd = ${JSON.stringify(join(REPO, "crates", "hypha-host-workers"))}`));
});

test("a region replaces measuring the host", () => {
  const d = deployment();
  d.cloudflare!.region = "aws:eu-central-1";
  assert.match(toml(d), /\[placement\]\nmode = "targeted"\nregion = "aws:eu-central-1"\n/);
});

test("media is R2's S3 endpoint, public files from the bucket's domain only with its zone", () => {
  const d = deployment();
  assert.doesNotMatch(toml(d, false), /HYPHA_S3_/);
  const signed = toml(d);
  assert.match(signed, /^HYPHA_S3_ENDPOINT = "https:\/\/a{32}\.r2\.cloudflarestorage\.com"$/m);
  assert.match(signed, /^HYPHA_S3_BUCKET = "hypha-media"$/m);
  assert.doesNotMatch(signed, /HYPHA_MEDIA_URL/);
  d.cloudflare!.zoneId = "z".repeat(32);
  assert.match(toml(d), /^HYPHA_MEDIA_URL = "https:\/\/media\.example\.com"$/m);
});

/** Tools that record what they're asked and answer as wrangler would. */
/** What `deployApp` was given, per app. */
type Deployed = { env: Record<string, string>; secrets: Record<string, string> };

function fakeTools(): {
  tools: Tools;
  calls: string[];
  secrets: string[];
  /** Each `wrangler deploy`'s configuration, in order. */
  configs: string[];
  hostEnvs: Record<string, string>[];
  apps: Record<string, Deployed>;
} {
  const calls: string[] = [];
  const secrets: string[] = [];
  const configs: string[] = [];
  const hostEnvs: Record<string, string>[] = [];
  const deployedApps: Record<string, Deployed> = {};
  let namespaces = 0;
  const tools: Tools = {
    async wrangler(args) {
      calls.push(`wrangler ${args.join(" ")}`);
      const command = args.slice(0, 3).join(" ");
      if (args[0] === "whoami")
        return JSON.stringify({ accounts: [{ id: "a".repeat(32), name: "Shop" }] });
      if (command.startsWith("hyperdrive create")) return `Created\n{ "id": "${"1".repeat(32)}" }`;
      if (command === "kv namespace list") return "[]";
      if (command === "kv namespace create") return `id = "${String(2 + namespaces++).repeat(32)}"`;
      if (command === "d1 list --json") return "[]";
      if (command.startsWith("d1 create"))
        return `✅ Created\n[[d1_databases]]\nbinding = "DB"\ndatabase_id = "${D1_ID}"`;
      if (args[0] === "deploy") {
        const file = args[args.indexOf("--secrets-file") + 1];
        secrets.push(await readFile(file, "utf8"));
        configs.push(await readFile(args[args.indexOf("--config") + 1], "utf8"));
      }
      return "";
    },
    async buildHost() {
      calls.push("build host");
      return "/bin/hypha-host-axum";
    },
    async host(_binary, args, env) {
      calls.push(`host ${args.join(" ")} DATABASE_URL=${env.DATABASE_URL}`);
      hostEnvs.push(env);
      return args[0] === "config" ? JSON.stringify({ slots: 100, bindings: BINDINGS }) : "";
    },
    async checkToolchain() {},
    now: () => 1_800_000_000_000,
    answers: async () => true,
    async reachable(target) {
      calls.push(`reach ${target}`);
      return true;
    },
    async connectStorefront() {
      calls.push("connect storefront");
      return {
        key: "hypha_pk_shop",
        webhookSecret: "hypha_as_hook",
        orderUrl: "https://shop.example.com/order/{order}?token={token}",
      };
    },
    async syncWebhook() {
      calls.push("sync webhook");
      return null;
    },
    async deployApp(app, env, secretsFile) {
      calls.push(`deploy app ${app}`);
      deployedApps[app] = { env, secrets: JSON.parse(await readFile(secretsFile, "utf8")) };
    },
    async prepareApp(entry) {
      calls.push(`prepare ${entry.name} ${entry.rev}`);
      const clone = await mkdtemp(join(tmpdir(), `hypha-app-${entry.name}-`));
      return { clone, profile: profileOf({ wrangler: MEILI_TEMPLATE, dockerfile: "EXPOSE 8787" }) };
    },
  };
  return { tools, calls, secrets, configs, hostEnvs, apps: deployedApps };
}

async function folder(d: Deployment, r2 = true): Promise<{ dir: string; env: EnvFile }> {
  const dir = await mkdtemp(join(tmpdir(), "hypha-cf-"));
  await saveDeployment(dir, d);
  await writeFile(join(dir, "hypha.config.toml"), "# defaults\n");
  const env = new EnvFile()
    .set("HYPHA_SECRET_KEY", "f".repeat(64))
    .set("DATABASE_URL", DB)
    .set("R2_ACCESS_KEY_ID", r2 ? "a".repeat(32) : "")
    .set("R2_SECRET_ACCESS_KEY", r2 ? "b".repeat(64) : "")
    .set("HYPHA_OWNER_EMAIL", "owner@example.com")
    .set("HYPHA_OWNER_PASSWORD", "pw")
    .set("CLOUDFLARE_API_TOKEN", "cf-token");
  return { dir, env };
}

test("a first deploy sets the database up, creates the resources with Hyperdrive's cache off, and deploys", async () => {
  const d = deployment();
  const { dir, env } = await folder(d);
  const { tools, calls, secrets } = fakeTools();
  await deployCloudflare(dir, d, env, tools);
  assert.deepEqual(calls, [
    "wrangler whoami --json",
    "build host",
    `host setup DATABASE_URL=${DB}`,
    `host config rate-limits DATABASE_URL=${DB}`,
    `wrangler hyperdrive create hypha --connection-string ${PLAIN} --sslmode require --caching-disabled`,
    "wrangler kv namespace list",
    "wrangler kv namespace create hypha-jobs",
    "wrangler r2 bucket create hypha-media",
    `wrangler r2 bucket cors set hypha-media --file ${join(dir, ".cors.json")} --force`,
    `wrangler deploy --config ${join(dir, "wrangler.toml")} --secrets-file ${join(dir, ".secrets.json")}`,
    `wrangler kv key put jobs:written 1800000000000 --namespace-id ${"2".repeat(32)} --remote`,
    "deploy app admin",
    "reach https://admin.example.com",
  ]);
  const { HYPHA_CALLER_KEY: callerKey, ...apiSecrets } = JSON.parse(secrets[0]);
  assert.deepEqual(apiSecrets, {
    HYPHA_SECRET_KEY: "f".repeat(64),
    HYPHA_S3_ACCESS_KEY_ID: "a".repeat(32),
    HYPHA_S3_SECRET_ACCESS_KEY: "b".repeat(64),
  });
  // The admin is deployed, so the API gets a caller key, made once and kept.
  assert.match(callerKey, /^[0-9a-f]{64}$/);
  assert.equal(
    EnvFile.parse(await readFile(join(dir, ".env"), "utf8")).get("HYPHA_CALLER_KEY"),
    callerKey,
  );
  // The secrets don't stay on disk; what was created is remembered.
  assert.deepEqual((await readdir(dir)).toSorted(), [
    ".env",
    "hypha.config.toml",
    "hypha.json",
    "wrangler.toml",
  ]);
  const saved = await loadDeployment(dir);
  assert.equal(saved.cloudflare?.accountId, "a".repeat(32));
  assert.equal(saved.cloudflare?.hyperdriveId, "1".repeat(32));
  assert.equal(saved.cloudflare?.kvId, "2".repeat(32));
});

test("a later deploy updates Hyperdrive (cache still off) and creates nothing again", async () => {
  const d = deployment();
  Object.assign(d.cloudflare!, {
    accountId: "a".repeat(32),
    hyperdriveId: "1".repeat(32),
    kvId: "2".repeat(32),
    zoneId: "z1",
  });
  const { dir, env } = await folder(d, false);
  const { tools, calls, secrets } = fakeTools();
  await deployCloudflare(dir, d, env, tools);
  assert.ok(
    calls.includes(
      `wrangler hyperdrive update ${"1".repeat(32)} --connection-string ${PLAIN} --sslmode require --caching-disabled`,
    ),
  );
  assert.ok(!calls.some((c) => / create /.test(c) || c.includes("kv namespace list")));
  // No R2 keys: no bucket, no media secrets.
  assert.ok(!calls.some((c) => c.includes(" r2 ")));
  assert.deepEqual(JSON.parse(secrets[0]), {
    HYPHA_SECRET_KEY: "f".repeat(64),
    HYPHA_CALLER_KEY: env.get("HYPHA_CALLER_KEY"),
  });
});

test("the Resend key goes to the Worker as a secret, and to setup", async () => {
  const d = deployment();
  Object.assign(d.cloudflare!, {
    accountId: "a".repeat(32),
    hyperdriveId: "1".repeat(32),
    kvId: "2".repeat(32),
  });
  const { dir, env } = await folder(d, false);
  env.set("HYPHA_RESEND_API_KEY", "re_123");
  const { tools, secrets, hostEnvs } = fakeTools();
  await deployCloudflare(dir, d, env, tools);
  assert.equal(JSON.parse(secrets[0]).HYPHA_RESEND_API_KEY, "re_123");
  assert.equal(hostEnvs[0].HYPHA_RESEND_API_KEY, "re_123");
});

test("Hyperdrive gets the URL without its query, TLS as --sslmode, and never a cache", () => {
  assert.deepEqual(hyperdriveOrigin(DB), [
    "--connection-string",
    PLAIN,
    "--sslmode",
    "require",
    "--caching-disabled",
  ]);
  assert.deepEqual(
    hyperdriveOrigin("postgres://u:p@db.example.com:6543/x?sslmode=prefer&application_name=a"),
    ["--connection-string", "postgres://u:p@db.example.com:6543/x", "--caching-disabled"],
  );
});

test("a failed Hyperdrive call doesn't print the database's password", async () => {
  const d = deployment();
  const { dir, env } = await folder(d, false);
  const { tools } = fakeTools();
  const wrangler = tools.wrangler.bind(tools);
  tools.wrangler = async (args, options) => {
    if (args[0] === "hyperdrive") throw new Error(`\`wrangler ${args.join(" ")}\` failed (exit 1)`);
    return wrangler(args, options);
  };
  await assert.rejects(deployCloudflare(dir, d, env, tools), (e: Error) => {
    assert.doesNotMatch(e.message, /secret/);
    assert.match(e.message, /hypha:\*\*\*@/);
    return true;
  });
});

test("a binding outside the deployment's block is refused, not spilled into another's", () => {
  const d = deployment();
  assert.throws(
    () =>
      wranglerToml(d, {
        dir: join(REPO, "hypha-deploy"),
        accountId: "a".repeat(32),
        databaseUrl: DB,
        hyperdriveId: "h".repeat(32),
        kvId: "k".repeat(32),
        config: "",
        bindings: [{ rule: "x", binding: "RL_X", slot: 100, limit: 1, period: 60 }],
        compatibilityDate: "2026-10-01",
        media: false,
      }),
    /outside a deployment's block of 100/,
  );
});

test("a bare environment keeps what Windows needs for DNS and TLS, and nothing else", () => {
  const env = systemEnv({
    PATH: "C:\\Windows",
    SystemRoot: "C:\\Windows",
    TEMP: "C:\\Temp",
    DATABASE_URL: "postgres://elsewhere",
    HYPHA_S3_ENDPOINT: "http://localhost:8333",
  });
  assert.deepEqual(env, { PATH: "C:\\Windows", SystemRoot: "C:\\Windows", TEMP: "C:\\Temp" });
});

test("the admin and storefront deploy after the API, bound to it, with the storefront connected", async () => {
  const d = deployment();
  d.domains.storefront = "shop.example.com";
  Object.assign(d.cloudflare!, {
    accountId: "a".repeat(32),
    hyperdriveId: "1".repeat(32),
    kvId: "2".repeat(32),
  });
  const { dir, env } = await folder(d, false);
  const { tools, calls, apps } = fakeTools();
  await deployCloudflare(dir, d, env, tools);
  const tail = calls.slice(
    calls.indexOf(
      `wrangler kv key put jobs:written 1800000000000 --namespace-id ${"2".repeat(32)} --remote`,
    ) + 1,
  );
  assert.deepEqual(tail, [
    "connect storefront",
    "wrangler kv namespace list",
    "wrangler kv namespace create hypha-cache",
    "deploy app admin",
    "deploy app storefront",
    "reach https://admin.example.com",
    "reach https://shop.example.com",
  ]);
  assert.deepEqual(apps.storefront.env, {
    CLOUDFLARE_API_TOKEN: "cf-token",
    CLOUDFLARE_ACCOUNT_ID: "a".repeat(32),
    HYPHA_CF_ACCOUNT_ID: "a".repeat(32),
    HYPHA_CF_WORKER: "hypha-storefront",
    HYPHA_CF_DOMAIN: "shop.example.com",
    HYPHA_CF_API_WORKER: "hypha-api",
    HYPHA_CF_CACHE_KV: "2".repeat(32),
  });
  // Both apps send the API the same caller key.
  const callerKey = env.get("HYPHA_CALLER_KEY");
  assert.match(callerKey ?? "", /^[0-9a-f]{64}$/);
  assert.deepEqual(apps.storefront.secrets, {
    HYPHA_API_URL: "https://api.example.com",
    HYPHA_CALLER_KEY: callerKey,
    STOREFRONT_KEY: "hypha_pk_shop",
    HYPHA_WEBHOOK_SECRET: "hypha_as_hook",
  });
  assert.deepEqual(apps.admin.secrets, {
    HYPHA_API_URL: "https://api.example.com",
    HYPHA_CALLER_KEY: callerKey,
  });
  assert.equal(apps.admin.env.HYPHA_CF_WORKER, "hypha-admin");
  // The keys are kept for the next deploy, and the cache's namespace is remembered.
  const kept = EnvFile.parse(await readFile(join(dir, ".env"), "utf8"));
  assert.equal(kept.get("STOREFRONT_KEY"), "hypha_pk_shop");
  assert.equal((await loadDeployment(dir)).cloudflare?.cacheKvId, "2".repeat(32));
  // No app's secrets stay on disk.
  assert.ok(!(await readdir(dir)).some((f) => f.startsWith(".secrets")));
});

test("a connected storefront keeps its keys and only checks its webhook", async () => {
  const d = deployment();
  d.domains.storefront = "shop.example.com";
  Object.assign(d.cloudflare!, {
    accountId: "a".repeat(32),
    hyperdriveId: "1".repeat(32),
    kvId: "2".repeat(32),
    cacheKvId: "3".repeat(32),
  });
  const { dir, env } = await folder(d, false);
  env.set("STOREFRONT_KEY", "hypha_pk_old").set("HYPHA_WEBHOOK_SECRET", "hypha_as_old");
  const { tools, calls, apps } = fakeTools();
  await deployCloudflare(dir, d, env, tools);
  assert.ok(calls.includes("sync webhook") && !calls.includes("connect storefront"));
  assert.equal(apps.storefront.secrets.STOREFRONT_KEY, "hypha_pk_old");
  assert.equal(apps.storefront.env.HYPHA_CF_CACHE_KV, "3".repeat(32));
});

test("without an API token, the apps can't deploy and nothing starts", async () => {
  const d = deployment();
  const { dir, env } = await folder(d, false);
  env.set("CLOUDFLARE_API_TOKEN", "");
  const { tools, calls } = fakeTools();
  await assert.rejects(deployCloudflare(dir, d, env, tools), /CLOUDFLARE_API_TOKEN/);
  assert.deepEqual(calls, []);
});

test("the API binds to each co-located app's Worker and names their hosts", () => {
  const file = wranglerToml(deployment(), {
    dir: join(REPO, "hypha-deploy"),
    accountId: "a".repeat(32),
    databaseUrl: DB,
    hyperdriveId: "h".repeat(32),
    kvId: "k".repeat(32),
    config: "",
    bindings: [],
    compatibilityDate: "2026-10-01",
    media: false,
    apps: [
      { host: "stripe.example.com", binding: "APP_STRIPE", worker: "hypha-app-stripe" },
      { host: "search.example.com", binding: "APP_MEILISEARCH", worker: "hypha-app-meilisearch" },
    ],
  });
  assert.match(file, /\[\[services\]\]\nbinding = "APP_STRIPE"\nservice = "hypha-app-stripe"\n/);
  assert.match(
    file,
    /\[\[services\]\]\nbinding = "APP_MEILISEARCH"\nservice = "hypha-app-meilisearch"\n/,
  );
  assert.match(
    file,
    /^HYPHA_APP_BINDINGS = "stripe\.example\.com=APP_STRIPE,search\.example\.com=APP_MEILISEARCH"$/m,
  );
  // Without apps, neither.
  assert.doesNotMatch(toml(deployment()), /HYPHA_APP_BINDINGS|\[\[services\]\]/);
});

test("a new app gets its D1 database and Worker after the API, then the API is bound to it", async () => {
  const d = deployment();
  Object.assign(d.cloudflare!, {
    accountId: "a".repeat(32),
    hyperdriveId: "1".repeat(32),
    kvId: "2".repeat(32),
  });
  d.apps = [
    { name: "meilisearch", git: "git@example.com:a/b.git", rev: REV, domain: "search.example.com" },
  ];
  const { dir, env } = await folder(d, false);
  const { tools, calls, secrets, configs } = fakeTools();
  await deployCloudflare(dir, d, env, tools);
  const apiDeploy = `wrangler deploy --config ${join(dir, "wrangler.toml")} --secrets-file ${join(dir, ".secrets.json")}`;
  const appDeploy = calls.find((c) => c.includes(".hypha-wrangler.toml"))!;
  assert.ok(appDeploy, "the app was deployed");
  assert.deepEqual(
    calls.filter(
      (c) => c.startsWith("prepare") || c.includes(" d1 ") || c.startsWith("wrangler deploy"),
    ),
    [
      `prepare meilisearch ${REV}`,
      apiDeploy,
      "wrangler d1 list --json",
      "wrangler d1 create hypha-app-meilisearch",
      appDeploy,
      apiDeploy,
    ],
  );
  assert.ok(calls.includes("reach https://search.example.com/health"));
  // The first API deploy can't bind a Worker that isn't there yet; the second does.
  assert.doesNotMatch(configs[0], /APP_MEILISEARCH/);
  assert.match(configs[2], /binding = "APP_MEILISEARCH"\nservice = "hypha-app-meilisearch"/);
  assert.match(configs[2], /^HYPHA_APP_BINDINGS = "search\.example\.com=APP_MEILISEARCH"$/m);
  // The app's own configuration: its template, made this deployment's.
  const app = configs[1];
  assert.match(app, /^name = "hypha-app-meilisearch"$/m);
  assert.match(app, /^main = "src\/hosts\/worker\.ts"$/m);
  assert.match(app, /^compatibility_flags = \["nodejs_compat"\]$/m);
  assert.match(app, /^routes = \[\{ pattern = "search\.example\.com", custom_domain = true \}\]$/m);
  assert.match(app, /^APP_URL = "https:\/\/search\.example\.com"$/m);
  assert.match(app, /^HYPHA_API_URLS = "https:\/\/api\.example\.com"$/m);
  assert.match(app, /^HYPHA_API_URL = "https:\/\/api\.example\.com"$/m);
  assert.match(app, new RegExp(`^database_id = "${D1_ID}"$`, "m"));
  assert.match(app, /\[\[services\]\]\nbinding = "HYPHA_API"\nservice = "hypha-api"/);
  // Its encryption key, made once and kept in .env, went as its secret.
  const key = EnvFile.parse(await readFile(join(dir, ".env"), "utf8")).get(
    "APP_MEILISEARCH_ENCRYPTION_KEY",
  );
  assert.match(key ?? "", /^[0-9a-f]{64}$/);
  assert.deepEqual(JSON.parse(secrets[1]), { APP_ENCRYPTION_KEY: key });
  const saved = await loadDeployment(dir);
  assert.deepEqual(saved.apps?.[0], { ...d.apps[0], d1Id: D1_ID, deployed: true });
  assert.ok(!(await readdir(dir)).some((f) => f.startsWith(".secrets")));
});

test("an app deployed before is bound from the API's first deploy, and nothing is made again", async () => {
  const d = deployment();
  Object.assign(d.cloudflare!, {
    accountId: "a".repeat(32),
    hyperdriveId: "1".repeat(32),
    kvId: "2".repeat(32),
  });
  d.apps = [
    { name: "meilisearch", git: "git@example.com:a/b.git", rev: REV, d1Id: D1_ID, deployed: true },
  ];
  const { dir, env } = await folder(d, false);
  env.set("APP_MEILISEARCH_ENCRYPTION_KEY", "e".repeat(64));
  const { tools, calls, configs, secrets } = fakeTools();
  await deployCloudflare(dir, d, env, tools);
  assert.ok(!calls.some((c) => c.includes(" d1 ")));
  assert.equal(configs.length, 2);
  assert.match(configs[0], /^HYPHA_APP_BINDINGS = "meilisearch\.example\.com=APP_MEILISEARCH"$/m);
  assert.deepEqual(JSON.parse(secrets[1]), { APP_ENCRYPTION_KEY: "e".repeat(64) });
});

test("an app without a wrangler template stops the deploy before anything changes", async () => {
  const d = deployment();
  d.apps = [{ name: "plain", git: "git@example.com:a/plain.git", rev: REV }];
  const { dir, env } = await folder(d, false);
  const { tools, calls } = fakeTools();
  tools.prepareApp = async () => ({ clone: dir, profile: profileOf({}) });
  await assert.rejects(deployCloudflare(dir, d, env, tools), /no wrangler template/);
  assert.ok(!calls.some((c) => c.includes("deploy") || c.startsWith("host")));
});
