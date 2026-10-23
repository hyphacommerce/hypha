import assert from "node:assert/strict";
import { mkdir, mkdtemp, readFile, readlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import { appStatuses, installLink, storeApps } from "./app-install.ts";
import {
  appDomain,
  appWranglerToml,
  checkout,
  cloneDir,
  composeApps,
  entryFor,
  normalizeApps,
  profileOf,
  type Runner,
} from "./apps.ts";
import { MEILI_TEMPLATE } from "./fixtures.ts";
import { writeServerApps } from "./commands/deploy.ts";
import { type Deployment, EnvFile, loadDeployment, saveDeployment } from "./deployment.ts";
import { appsCaddy, caddyfile } from "./templates.ts";

const REV = "0123456789abcdef0123456789abcdef01234567";

/** The Stripe app's template and Dockerfile, as its repository has them (abridged). */
const STRIPE_TEMPLATE = `# The Worker's configuration: copy to wrangler.toml (git-ignored) and fill in.
name = "hypha-app-stripe"
main = "src/entry/worker.ts"
compatibility_date = "2026-10-01"

[vars]
APP_URL = "https://stripe.apps.example.com"
HYPHA_API_ALLOWLIST = ""

[[d1_databases]]
binding = "DB"
database_name = "hypha-app-stripe"
database_id = "00000000-0000-0000-0000-000000000000"

[observability]
enabled = true
`;
const STRIPE_DOCKERFILE = `FROM node:24-trixie-slim
WORKDIR /app
VOLUME ["/data"]
EXPOSE 3000
CMD ["node", "server.js"]`;
const MEILI_DOCKERFILE = `FROM node:24-slim
VOLUME /data
EXPOSE 8787`;

function vps(): Deployment {
  return {
    version: 1,
    target: "vps",
    ssh: "root@203.0.113.7",
    remoteDir: "/opt/hypha",
    buildOn: "server",
    scheme: "https",
    domain: "example.com",
    domains: { api: "api.example.com", media: "media.example.com", admin: "admin.example.com" },
  };
}

const stripe = {
  name: "stripe",
  git: "git@github.com:hyphacommerce/app-stripe.git",
  rev: REV,
};
const meili = {
  name: "meilisearch",
  git: "git@github.com:hyphacommerce/app-meilisearch.git",
  rev: REV,
  domain: "search.example.com",
};

test("a first-party app's name stands for its repository, unpinned", () => {
  assert.deepEqual(normalizeApps(["stripe"], "hypha.json"), [
    { name: "stripe", git: "git@github.com:hyphacommerce/app-stripe.git" },
  ]);
  assert.deepEqual(normalizeApps([{ name: "meilisearch", rev: REV }], "hypha.json"), [
    { name: "meilisearch", git: "git@github.com:hyphacommerce/app-meilisearch.git", rev: REV },
  ]);
});

test("an unknown name, a short commit, a twice-listed app or the SDK's link name is refused", () => {
  assert.throws(() => normalizeApps(["paypal"], "hypha.json"), /isn't a first-party app/);
  assert.throws(
    () => normalizeApps([{ name: "xy", git: "git@e.com:x.git", rev: "abc1234" }], "hypha.json"),
    /full commit ID/,
  );
  assert.throws(() => normalizeApps(["stripe", "stripe"], "hypha.json"), /listed twice/);
  assert.throws(
    () => normalizeApps([{ name: "hypha", git: "git@e.com:x.git" }], "hypha.json"),
    /taken by the link/,
  );
});

test("an app's domain in hypha.json is lowercased, and one that isn't a domain is refused", () => {
  assert.deepEqual(normalizeApps([{ name: "stripe", domain: " Pay.Example.COM " }], "hypha.json"), [
    {
      name: "stripe",
      git: "git@github.com:hyphacommerce/app-stripe.git",
      domain: "pay.example.com",
    },
  ]);
  for (const domain of [
    "https://pay.example.com",
    "pay.example.com:8443",
    "pay.example.com/x",
    "pay",
    "",
    42,
  ]) {
    assert.throws(
      () => normalizeApps([{ name: "stripe", domain }], "hypha.json"),
      /hypha\.json: app stripe: `domain` .* isn't a domain name/,
    );
  }
});

test("an app from a git URL is named after its repository, without the hypha-app- prefix", () => {
  assert.deepEqual(entryFor("git@github.com:acme/hypha-app-reviews.git"), {
    name: "reviews",
    git: "git@github.com:acme/hypha-app-reviews.git",
  });
  assert.equal(entryFor("https://github.com/acme/Loyalty/").name, "loyalty");
  assert.equal(entryFor("git@github.com:acme/x.git", "points").name, "points");
  assert.throws(() => entryFor("paypal"), /first-party app .* or a git URL/);
});

test("an app's domain is <name>.<the deployment's domain> unless it has its own", () => {
  const d = vps();
  assert.equal(appDomain(d, stripe), "stripe.example.com");
  assert.equal(appDomain(d, meili), "search.example.com");
  // A deployment from before `domain`: the API's domain without its first label.
  delete d.domain;
  assert.equal(appDomain(d, stripe), "stripe.example.com");
});

test("what the CLI needs is read from the app's repository", () => {
  assert.deepEqual(
    profileOf({
      dockerfile: STRIPE_DOCKERFILE,
      wrangler: STRIPE_TEMPLATE,
      packageJson: JSON.stringify({ scripts: { "build:node": "node scripts/build-node.ts" } }),
    }),
    {
      port: 3000,
      dataDir: "/data",
      wrangler: STRIPE_TEMPLATE,
      apiBinding: false,
      allowlistVar: "HYPHA_API_ALLOWLIST",
      nodeBuild: "build:node",
    },
  );
  const meiliProfile = profileOf({ dockerfile: MEILI_DOCKERFILE, wrangler: MEILI_TEMPLATE });
  assert.equal(meiliProfile.port, 8787);
  assert.equal(meiliProfile.dataDir, "/data");
  // Its template offers the binding to the API (commented out until deployed alongside).
  assert.equal(meiliProfile.apiBinding, true);
  assert.equal(meiliProfile.allowlistVar, "HYPHA_API_URLS");
  assert.deepEqual(profileOf({}), { port: 3000, apiBinding: false });
});

test("the app's Worker keeps its template and becomes this deployment's", () => {
  const file = appWranglerToml(STRIPE_TEMPLATE, {
    worker: "shop-app-stripe",
    accountId: "a".repeat(32),
    domain: "stripe.example.com",
    appUrl: "https://stripe.example.com",
    apiUrl: "https://api.example.com",
    d1: { name: "shop-app-stripe", id: "11111111-2222-3333-4444-555555555555" },
    allowlistVar: "HYPHA_API_ALLOWLIST",
  });
  assert.match(file, /^name = "shop-app-stripe"$/m);
  assert.match(file, /^main = "src\/entry\/worker\.ts"$/m);
  // Top-level keys before the first table, so TOML keeps them top-level.
  const routes = file.indexOf("routes = ");
  assert.ok(routes > 0 && routes < file.indexOf("[vars]"));
  assert.match(file, /^account_id = "a{32}"$/m);
  assert.match(file, /^workers_dev = false$/m);
  assert.match(file, /^APP_URL = "https:\/\/stripe\.example\.com"$/m);
  assert.match(file, /^HYPHA_API_ALLOWLIST = "https:\/\/api\.example\.com"$/m);
  assert.match(file, /^database_name = "shop-app-stripe"$/m);
  assert.match(file, /^database_id = "11111111-2222-3333-4444-555555555555"$/m);
  assert.match(file, /\[observability\]\nenabled = true/);
  // The Stripe app takes no binding to the API.
  assert.doesNotMatch(file, /HYPHA_API_URL|\[\[services\]\]/);
});

test("an app's template without a D1 database can't be deployed", () => {
  assert.throws(
    () =>
      appWranglerToml('name = "x"\nmain = "w.ts"\n', {
        worker: "w",
        accountId: "a",
        domain: "x.example.com",
        appUrl: "https://x.example.com",
        apiUrl: "https://api.example.com",
        d1: { name: "w", id: "i" },
      }),
    /d1_databases/,
  );
});

test("on a server each app is a container on the network, with its key and its data kept", () => {
  const d = vps();
  const file = composeApps(d, [
    {
      entry: stripe,
      profile: profileOf({ dockerfile: STRIPE_DOCKERFILE, wrangler: STRIPE_TEMPLATE }),
    },
    {
      entry: meili,
      profile: profileOf({ dockerfile: MEILI_DOCKERFILE, wrangler: MEILI_TEMPLATE }),
    },
  ]);
  assert.match(
    file,
    /  app-stripe:\n    image: hypha-app-stripe:current\n    restart: unless-stopped\n    environment:\n      APP_URL: https:\/\/stripe\.example\.com\n      APP_ENCRYPTION_KEY: \$\{APP_STRIPE_ENCRYPTION_KEY:\?\}\n      PORT: "3000"\n      HYPHA_API_ALLOWLIST: https:\/\/api\.example\.com\n    volumes:\n      - app-stripe-data:\/data\n    networks: \[hypha\]/,
  );
  assert.match(file, /APP_URL: https:\/\/search\.example\.com/);
  assert.match(file, /HYPHA_API_URLS: https:\/\/api\.example\.com/);
  assert.match(
    file,
    /  caddy:\n    volumes:\n      - \.\/apps\.caddy:\/etc\/caddy\/apps\.caddy:ro\n/,
  );
  assert.match(file, /^volumes:\n  app-stripe-data:\n  app-meilisearch-data:\n$/m);
  // On a server, Hypha calls the apps at their public addresses: no aliases.
  assert.doesNotMatch(file, /aliases/);
});

test("on this machine the API's and the apps' domains lead to Caddy inside the network", () => {
  const d: Deployment = {
    ...vps(),
    target: "local",
    scheme: "http",
    domain: "hypha.localhost",
    domains: { api: "api.hypha.localhost", media: "media.hypha.localhost" },
  };
  const file = composeApps(d, [{ entry: stripe, profile: profileOf({}) }]);
  assert.match(
    file,
    /        aliases:\n          - \$\{MEDIA_HOST:\?\}\n          - api\.hypha\.localhost\n          - stripe\.hypha\.localhost\n/,
  );
  assert.match(file, /APP_URL: http:\/\/stripe\.hypha\.localhost/);
});

test("Caddy serves each app on its domain, and the Caddyfile imports them", () => {
  const d = vps();
  const sites = appsCaddy(d, [
    { entry: stripe, profile: profileOf({ dockerfile: STRIPE_DOCKERFILE }) },
    { entry: meili, profile: profileOf({ dockerfile: MEILI_DOCKERFILE }) },
  ]);
  assert.match(
    sites,
    /https:\/\/stripe\.example\.com \{\n\tencode zstd gzip\n\treverse_proxy app-stripe:3000\n\}/,
  );
  assert.match(
    sites,
    /https:\/\/search\.example\.com \{\n\tencode zstd gzip\n\treverse_proxy app-meilisearch:8787\n\}/,
  );
  assert.match(caddyfile(d), /^import \/etc\/caddy\/apps\.caddy$/m);
});

test("a server's files gain the apps: compose reads both files, keys are made once, an old Caddyfile imports them", async () => {
  const dir = await mkdtemp(join(tmpdir(), "hypha-apps-"));
  const d = { ...vps(), apps: [stripe] };
  await writeFile(
    join(dir, "Caddyfile"),
    "https://api.example.com {\n\treverse_proxy api:3000\n}\n",
  );
  await writeFile(join(dir, ".env"), new EnvFile().set("HYPHA_SECRET_KEY", "k").toString());
  const coLocated = [{ entry: stripe, profile: profileOf({ dockerfile: STRIPE_DOCKERFILE }) }];
  await writeServerApps(dir, d, coLocated);
  await writeServerApps(dir, d, coLocated);
  const caddy = await readFile(join(dir, "Caddyfile"), "utf8");
  assert.equal(caddy.split("import /etc/caddy/apps.caddy").length, 2, "imported once");
  assert.ok(caddy.startsWith("https://api.example.com {"));
  const env = EnvFile.parse(await readFile(join(dir, ".env"), "utf8"));
  assert.equal(env.get("COMPOSE_FILE"), "compose.yaml:compose.apps.yaml");
  assert.equal(env.get("COMPOSE_PATH_SEPARATOR"), ":");
  const key = env.get("APP_STRIPE_ENCRYPTION_KEY");
  assert.match(key ?? "", /^[0-9a-f]{64}$/);
  // A server's API calls only public addresses.
  assert.equal(env.get("HYPHA_ALLOW_PRIVATE_CALLBACKS"), undefined);
  // Its public address (HYPHA_PUBLIC_URL), for a `.env` from before it.
  assert.equal(env.get("API_URL"), "https://api.example.com");
  assert.match(await readFile(join(dir, "compose.apps.yaml"), "utf8"), /app-stripe:/);
  assert.match(await readFile(join(dir, "apps.caddy"), "utf8"), /reverse_proxy app-stripe:3000/);
  // With no apps, the files stay, empty of them: the container goes on the next `up`.
  await writeServerApps(dir, { ...d, apps: [] }, []);
  assert.doesNotMatch(await readFile(join(dir, "compose.apps.yaml"), "utf8"), /app-stripe/);
  assert.equal(
    EnvFile.parse(await readFile(join(dir, ".env"), "utf8")).get("APP_STRIPE_ENCRYPTION_KEY"),
    key,
  );
});

test("a checkout clones next to a link to this checkout, at the pinned commit, then installs", async () => {
  const dir = await mkdtemp(join(tmpdir(), "hypha-clone-"));
  const calls: string[] = [];
  const runner: Runner = async (program, args, options = {}) => {
    calls.push(`${options.cwd ? `[${options.cwd}] ` : ""}${program} ${args.join(" ")}`);
    if (program === "git" && args[0] === "clone")
      await mkdir(join(args[3], ".git"), { recursive: true });
    if (program === "git" && args[0] === "cat-file") throw new Error("missing");
    return "";
  };
  const clone = await checkout(dir, stripe, runner);
  assert.equal(clone, cloneDir(dir, stripe));
  assert.equal(
    await readlink(join(dir, ".build", "apps", "hypha")),
    join(import.meta.dirname, "..", "..", ".."),
  );
  const tail = calls.filter(
    (c) => !c.includes("pnpm install --frozen-lockfile") || c.startsWith(`[${clone}]`),
  );
  assert.deepEqual(tail, [
    `git clone --quiet ${stripe.git} ${clone}`,
    `[${clone}] git cat-file -e ${REV}^{commit}`,
    `[${clone}] git fetch --quiet origin ${REV}`,
    `[${clone}] git -c advice.detachedHead=false checkout --quiet --force ${REV}`,
    `[${clone}] pnpm install --frozen-lockfile`,
  ]);
  // A second checkout at a known commit neither clones nor fetches.
  calls.length = 0;
  const known: Runner = async (program, args, options) => {
    if (program === "git" && args[0] === "cat-file") return "";
    return runner(program, args, options);
  };
  await checkout(dir, stripe, known);
  assert.ok(!calls.some((c) => c.includes("git clone") || c.includes("git fetch")));
});

test("an unpinned app isn't checked out", async () => {
  await assert.rejects(checkout("/tmp", { name: "stripe", git: "x" }), /isn't pinned/);
});

test("hypha.json's apps survive a round trip", async () => {
  const dir = await mkdtemp(join(tmpdir(), "hypha-state-"));
  const d = { ...vps(), apps: [stripe, meili] };
  await saveDeployment(dir, d);
  assert.deepEqual((await loadDeployment(dir)).apps, [stripe, meili]);
});

test("the install link names this deployment's API", () => {
  assert.equal(
    installLink(vps(), stripe),
    "https://stripe.example.com/install?api=https%3A%2F%2Fapi.example.com",
  );
});

test("the store's apps are read page by page; an app is installed only with a grant in force", async () => {
  const pages: Record<string, unknown> = {
    "/admin/apps?limit=100": {
      items: [{ key: "app_storefront", installed: false, enabled: true }],
      next_cursor: "c2",
    },
    "/admin/apps?limit=100&cursor=c2": {
      items: [{ key: "app_stripe", installed: true, enabled: true }],
      next_cursor: null,
    },
  };
  const staff = {
    // oxlint-disable-next-line typescript/no-unsafe-type-assertion -- the test names each page
    call: async <T>(_method: string, path: string) => pages[path] as T,
  };
  assert.deepEqual([...(await storeApps(staff)).keys()], ["app_storefront", "app_stripe"]);
  const d = { ...vps(), apps: [stripe, meili] };
  const manifests: Record<string, unknown> = {
    "https://stripe.example.com/manifest.json": { key: "app_stripe" },
  };
  const statuses = await appStatuses(d, staff, async (target) => manifests[target]);
  assert.deepEqual(
    statuses.map((s) => [s.entry.name, s.key, s.reachable, s.installed]),
    [
      ["stripe", "app_stripe", true, true],
      // Its manifest didn't answer: known by its name's key, and not installed.
      ["meilisearch", "app_meilisearch", false, false],
    ],
  );
  // Without a session, whether they're installed is unknown.
  const unknown = await appStatuses(d, undefined, async (target) => manifests[target]);
  assert.equal(unknown[0].installed, undefined);
});
