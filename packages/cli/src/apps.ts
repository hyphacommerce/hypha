// Apps deployed alongside Hypha: first-party apps (Stripe, Meilisearch) or any app built
// the same way, each from its own repository at a pinned commit. They get no shortcut
// into the store: the deploy runs them, then a staff member installs each one through its
// consent page like any app's.
//
// Each is cloned here, with this machine's git credentials, into
// `<deployment>/.build/apps/<name>`, next to `<deployment>/.build/apps/hypha`, a link to
// this Hypha checkout: the apps link Hypha's SDK packages as `../hypha/packages/*`
// (until those are on a registry), so that path has to lead here. Then `pnpm install`,
// and per target:
//
// - Cloudflare: the app's Worker (`<name>-app-<app>`), on its custom domain, with its D1
//   database and `APP_ENCRYPTION_KEY`, from its own wrangler template; the API binds to
//   it (`HYPHA_APP_BINDINGS`, so Hypha's calls to it stay on Cloudflare's network), and
//   it to the API (`HYPHA_API`) when its template supports that;
// - a server: `pnpm build:node` (one bundle, the SDK inlined) and the app's Dockerfile,
//   built where the API is, run as `app-<name>` in `compose.apps.yaml` and served by
//   Caddy on its domain (`apps.caddy`).
//
// What the CLI needs to know of an app comes from its repository (`profileOf`): the
// port and data folder its Dockerfile declares, its wrangler template, whether that
// names the `HYPHA_API` binding, and which variable lists the APIs allowed to install it.

import { randomBytes } from "node:crypto";
import { mkdir, readFile, readlink, rm, stat, symlink } from "node:fs/promises";
import { join } from "node:path";

import type { AppEntry, Deployment } from "./deployment.ts";
import { url } from "./deployment.ts";
import { run } from "./shell.ts";
import { REPO, requireRepo } from "./templates.ts";

export type { AppEntry } from "./deployment.ts";

/** The first-party apps, by name. */
export const FIRST_PARTY: Record<string, string> = {
  stripe: "git@github.com:hyphacommerce/app-stripe.git",
  meilisearch: "git@github.com:hyphacommerce/app-meilisearch.git",
};

/** An app's name: a DNS label and part of resource names. `hypha` is the SDK's link. */
const NAME = /^[a-z][a-z0-9-]{0,30}[a-z0-9]$/;

export function checkAppName(name: string): string | undefined {
  if (!NAME.test(name)) return "lowercase letters, digits and dashes (2 to 32)";
  if (name === "hypha") return "`hypha` is taken by the link to this checkout";
  return undefined;
}

/** A full commit ID. */
const REV = /^[0-9a-f]{40}$/;

/** A domain name, lowercase (the API Worker's `HYPHA_APP_BINDINGS` takes no other). */
const DOMAIN = /^(?=.{1,253}$)([a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z][a-z0-9-]{0,62}$/;

/** `domain` as an app's domain: trimmed and lowercased, or `undefined` if it isn't one. */
export function appDomainOf(domain: string): string | undefined {
  const lower = domain.trim().toLowerCase();
  return DOMAIN.test(lower) ? lower : undefined;
}

/** `hypha.json`'s `apps`, each a name (a first-party app) or an entry, checked. */
export function normalizeApps(raw: unknown, where: string): AppEntry[] {
  if (!Array.isArray(raw)) throw new Error(`${where}: \`apps\` is a list`);
  const out: AppEntry[] = [];
  for (const item of raw) {
    let entry: AppEntry;
    if (typeof item === "string") {
      const git = FIRST_PARTY[item];
      if (!git)
        throw new Error(
          `${where}: ${JSON.stringify(item)} isn't a first-party app (${Object.keys(FIRST_PARTY).join(", ")}): give {name, git, rev}`,
        );
      entry = { name: item, git };
    } else if (item && typeof item === "object") {
      // oxlint-disable-next-line typescript/no-unsafe-type-assertion -- each field is checked next
      const o = item as Partial<AppEntry>;
      const git = o.git ?? (o.name ? FIRST_PARTY[o.name] : undefined);
      if (typeof o.name !== "string" || typeof git !== "string")
        throw new Error(`${where}: an app needs \`name\` and \`git\``);
      entry = { ...o, name: o.name, git };
    } else {
      throw new Error(`${where}: an app is a name or {name, git, rev}`);
    }
    const wrong = checkAppName(entry.name);
    if (wrong) throw new Error(`${where}: app ${JSON.stringify(entry.name)}: ${wrong}`);
    if (entry.domain !== undefined) {
      const domain = typeof entry.domain === "string" ? appDomainOf(entry.domain) : undefined;
      if (!domain)
        throw new Error(
          `${where}: app ${entry.name}: \`domain\` ${JSON.stringify(entry.domain)} isn't a domain name (like stripe.example.com: no scheme, port or path)`,
        );
      entry.domain = domain;
    }
    if (entry.rev !== undefined && !REV.test(entry.rev))
      throw new Error(`${where}: app ${entry.name}: \`rev\` is a full commit ID (40 hex digits)`);
    if (out.some((a) => a.name === entry.name))
      throw new Error(`${where}: app ${entry.name} is listed twice`);
    out.push(entry);
  }
  return out;
}

/** What `hypha apps add` takes: a first-party name, or a repository (its name from the
 * repository's, without an `app-` or `hypha-app-` prefix, unless `name` is given). */
export function entryFor(spec: string, name?: string): AppEntry {
  const git = FIRST_PARTY[spec] ?? spec;
  if (git === spec && !/^(git@|https:\/\/|ssh:\/\/|file:\/\/|\/)/.test(spec)) {
    throw new Error(
      `${spec} isn't a first-party app (${Object.keys(FIRST_PARTY).join(", ")}) or a git URL`,
    );
  }
  const derived =
    name ??
    (FIRST_PARTY[spec]
      ? spec
      : (spec
          .replace(/\/+$/, "")
          .split(/[/:]/)
          .pop()
          ?.replace(/\.git$/, "")
          .replace(/^(hypha-)?app-/, "")
          .toLowerCase() ?? ""));
  const wrong = checkAppName(derived);
  if (wrong) throw new Error(`the app's name, ${JSON.stringify(derived)}: ${wrong} (pass --name)`);
  return { name: derived, git };
}

/** The domain the deployment's others go under. */
export function apex(d: Deployment): string {
  if (d.domain) return d.domain;
  const labels = d.domains.api.split(".");
  return labels.length > 2 ? labels.slice(1).join(".") : d.domains.api;
}

export const appDomain = (d: Deployment, a: AppEntry): string => a.domain ?? `${a.name}.${apex(d)}`;
export const appUrl = (d: Deployment, a: AppEntry): string => url(d, appDomain(d, a));

/** `stripe-payments` → `STRIPE_PAYMENTS`. */
const upper = (name: string) => name.toUpperCase().replaceAll("-", "_");

/** Its `APP_ENCRYPTION_KEY`, in the deployment's `.env`. */
export const encryptionKeyVar = (a: AppEntry) => `APP_${upper(a.name)}_ENCRYPTION_KEY`;
/** Matches every app's key, for `init --force` to keep. */
export const ENCRYPTION_KEY_VAR = /^APP_[A-Z0-9_]+_ENCRYPTION_KEY$/;
/** The API Worker's binding to its Worker. */
export const bindingName = (a: AppEntry) => `APP_${upper(a.name)}`;
/** Its Worker and D1 database on Cloudflare. */
export const appWorkerName = (d: Deployment, a: AppEntry) => `${d.cloudflare!.name}-app-${a.name}`;
/** Its container and image on a server. */
export const service = (a: AppEntry) => `app-${a.name}`;
export const imageTag = (a: AppEntry) => `hypha-app-${a.name}:current`;

/** A new `APP_ENCRYPTION_KEY`: 64 hex characters. Lost, the app loses what it sealed. */
export function newEncryptionKey(): string {
  return randomBytes(32).toString("hex");
}

// ---------- what the app's repository says ----------

/** What the CLI reads from an app's repository. */
export type AppProfile = {
  /** The port its Node server listens on (the Dockerfile's `EXPOSE`; 3000 without). */
  port: number;
  /** The folder its data lives in (the Dockerfile's `VOLUME`), kept in a volume. */
  dataDir?: string;
  /** Its wrangler template's text (`wrangler.example.toml` or `wrangler.toml.example`). */
  wrangler?: string;
  /** Its Worker takes a Service Binding to the API as `HYPHA_API` with `HYPHA_API_URL`. */
  apiBinding: boolean;
  /** The variable naming the APIs allowed to install it, set to this deployment's. */
  allowlistVar?: string;
  /** Builds the Node bundle the Dockerfile copies (`build:node`). */
  nodeBuild?: string;
};

/** The variables apps take the allowed APIs' origins in, as the first-party apps name it. */
const ALLOWLIST_VARS = ["HYPHA_API_ALLOWLIST", "HYPHA_API_URLS"];

export function profileOf(files: {
  dockerfile?: string;
  wrangler?: string;
  packageJson?: string;
}): AppProfile {
  const expose = /^\s*EXPOSE\s+(\d+)/m.exec(files.dockerfile ?? "")?.[1];
  const volume = /^\s*VOLUME\s+(?:\[\s*"([^"]+)"|(\S+))/m.exec(files.dockerfile ?? "");
  // oxlint-disable-next-line typescript/no-unsafe-type-assertion -- the app's package.json; only `scripts` is read
  const scripts = (files.packageJson ? JSON.parse(files.packageJson) : {}) as {
    scripts?: Record<string, string>;
  };
  const wrangler = files.wrangler;
  return {
    port: expose ? Number(expose) : 3000,
    ...(volume ? { dataDir: volume[1] ?? volume[2] } : {}),
    ...(wrangler ? { wrangler } : {}),
    apiBinding: Boolean(wrangler && /binding\s*=\s*"HYPHA_API"/.test(wrangler)),
    ...(() => {
      const found = ALLOWLIST_VARS.find((v) =>
        new RegExp(`^\\s*${v}\\s*=`, "m").test(wrangler ?? ""),
      );
      return found ? { allowlistVar: found } : {};
    })(),
    ...(scripts.scripts?.["build:node"] ? { nodeBuild: "build:node" } : {}),
  };
}

const read = (path: string) => readFile(path, "utf8").catch(() => undefined);

export async function readProfile(clone: string): Promise<AppProfile> {
  return profileOf({
    dockerfile: await read(join(clone, "Dockerfile")),
    wrangler:
      (await read(join(clone, "wrangler.example.toml"))) ??
      (await read(join(clone, "wrangler.toml.example"))),
    packageJson: await read(join(clone, "package.json")),
  });
}

// ---------- the Worker's configuration ----------

export type AppWorkerInput = {
  worker: string;
  accountId: string;
  domain: string;
  appUrl: string;
  apiUrl: string;
  /** The API Worker, when the app takes a binding to it. */
  apiWorker?: string;
  d1: { name: string; id: string };
  allowlistVar?: string;
};

/** A TOML string: JSON's escapes are TOML's. */
const str = (value: string) => JSON.stringify(value);

/**
 * The app's `wrangler.toml`, from its own template (which keeps its entry point, flags,
 * crons and other variables): its name, account and custom domain, `APP_URL`, the
 * allowed API, the D1 database, and the binding to the API when it supports one.
 */
export function appWranglerToml(template: string, o: AppWorkerInput): string {
  const lines = template.split("\n");
  const set = (pattern: RegExp, line: string, what: string) => {
    const i = lines.findIndex((l) => pattern.test(l));
    if (i < 0) throw new Error(`the app's wrangler template has no ${what}`);
    lines[i] = line;
    return i;
  };
  set(/^name\s*=/, `name = ${str(o.worker)}`, "`name`");
  const main = lines.findIndex((l) => /^main\s*=/.test(l));
  if (main < 0) throw new Error("the app's wrangler template has no `main`");
  // Top-level keys, before the first table.
  const firstTable = lines.findIndex((l) => l.startsWith("["));
  lines.splice(
    firstTable < 0 ? lines.length : firstTable,
    0,
    `account_id = ${str(o.accountId)}`,
    "workers_dev = false",
    `routes = [{ pattern = ${str(o.domain)}, custom_domain = true }]`,
    "",
  );
  let vars = lines.findIndex((l) => /^\[vars\]\s*$/.test(l));
  if (vars < 0) {
    lines.push("", "[vars]");
    vars = lines.length - 1;
  }
  const setVar = (key: string, value: string) => {
    const i = lines.findIndex((l) => new RegExp(`^${key}\\s*=`).test(l));
    if (i >= 0) lines[i] = `${key} = ${str(value)}`;
    else lines.splice(vars + 1, 0, `${key} = ${str(value)}`);
  };
  setVar("APP_URL", o.appUrl);
  if (o.allowlistVar) setVar(o.allowlistVar, new URL(o.apiUrl).origin);
  if (o.apiWorker) setVar("HYPHA_API_URL", o.apiUrl);
  const d1 = lines.findIndex((l) => l.startsWith("[[d1_databases]]"));
  if (d1 < 0) throw new Error("the app's wrangler template has no `[[d1_databases]]`");
  const inD1 = (pattern: RegExp, line: string) => {
    const end = lines.findIndex((l, i) => i > d1 && l.startsWith("["));
    const i = lines.findIndex((l, j) => j > d1 && (end < 0 || j < end) && pattern.test(l));
    if (i < 0) throw new Error("the app's wrangler template's D1 database lacks a key");
    lines[i] = line;
  };
  inD1(/^database_name\s*=/, `database_name = ${str(o.d1.name)}`);
  inD1(/^database_id\s*=/, `database_id = ${str(o.d1.id)}`);
  while (lines.at(-1) === "") lines.pop();
  if (o.apiWorker) {
    lines.push(
      "",
      "# Calls to HYPHA_API_URL go through this Service Binding.",
      "[[services]]",
      'binding = "HYPHA_API"',
      `service = ${str(o.apiWorker)}`,
    );
  }
  return [
    "# Written by `hypha deploy` from the app's wrangler template and hypha.json on every",
    "# deploy: change those, not this file.",
    ...lines,
    "",
  ].join("\n");
}

/** The file it's written to, in the app's clone (`main` is relative to it). */
export const APP_WRANGLER_FILE = ".hypha-wrangler.toml";

// ---------- a server: compose and Caddy ----------

/** The co-located apps on a server: each a container on the deployment's network. */
export function composeApps(
  d: Deployment,
  apps: { entry: AppEntry; profile: AppProfile }[],
): string {
  const out = [
    "# Written by `hypha deploy` from hypha.json's apps on every",
    "# deploy: change those, not this file. Compose reads it after compose.yaml",
    "# (COMPOSE_FILE in .env).",
    "",
    "services:",
  ];
  const volumes: string[] = [];
  for (const { entry, profile } of apps) {
    const name = service(entry);
    out.push(
      `  ${name}:`,
      `    image: ${imageTag(entry)}`,
      "    restart: unless-stopped",
      "    environment:",
      `      APP_URL: ${appUrl(d, entry)}`,
      `      APP_ENCRYPTION_KEY: \${${encryptionKeyVar(entry)}:?}`,
      `      PORT: "${profile.port}"`,
      ...(profile.allowlistVar ? [`      ${profile.allowlistVar}: ${url(d, d.domains.api)}`] : []),
    );
    if (profile.dataDir) {
      volumes.push(`${name}-data`);
      out.push("    volumes:", `      - ${name}-data:${profile.dataDir}`);
    }
    out.push("    networks: [hypha]");
  }
  // Caddy serves the apps' sites (`apps.caddy`, imported by the Caddyfile).
  out.push("  caddy:", "    volumes:", "      - ./apps.caddy:/etc/caddy/apps.caddy:ro");
  if (d.target === "local") {
    // On this machine `*.localhost` is every container's own loopback: inside the
    // network, the API's and the apps' domains lead to Caddy instead, so the API reaches
    // the apps (their manifests, extension points) and they reach the API (OAuth).
    out.push(
      "    networks:",
      "      hypha:",
      "        aliases:",
      "          - ${MEDIA_HOST:?}",
      `          - ${d.domains.api}`,
      ...apps.map(({ entry }) => `          - ${appDomain(d, entry)}`),
    );
  }
  if (volumes.length > 0) out.push("", "volumes:", ...volumes.map((v) => `  ${v}:`));
  return `${out.join("\n")}\n`;
}

/** The file Compose reads after `compose.yaml`, and Caddy's sites for the apps. */
export const COMPOSE_APPS_FILE = "compose.apps.yaml";
export const APPS_CADDY_FILE = "apps.caddy";
/** The Caddyfile's line that reads `apps.caddy`. */
export const CADDY_IMPORT = "import /etc/caddy/apps.caddy";

// ---------- building ----------

/** The SDK's link next to the clones, and each clone's folder. */
export const buildRoot = (dir: string) => join(dir, ".build", "apps");
export const cloneDir = (dir: string, a: AppEntry) => join(buildRoot(dir), a.name);

/** Runs a program; tests record instead. */
export type Runner = (
  program: string,
  args: string[],
  options?: { cwd?: string; capture?: boolean },
) => Promise<string>;

const runHere: Runner = (program, args, options = {}) => run(program, args, options);

/** The commit `git`'s default branch (or `ref`) is at, asked of the remote. */
export async function remoteHead(git: string, ref = "HEAD", runner: Runner = runHere) {
  const out = await runner("git", ["ls-remote", git, ref], { capture: true });
  const rev = /^([0-9a-f]{40})\s/m.exec(out)?.[1];
  if (!rev) throw new Error(`${git} has no ${ref}`);
  return rev;
}

/** `git` at commit `rev` in `clone`: cloned the first time, fetched when the commit is new
 * to it, and left as it is when it's there already. With this machine's git credentials. */
export async function cloneAt(
  git: string,
  rev: string,
  clone: string,
  runner: Runner = runHere,
): Promise<void> {
  if (!(await exists(join(clone, ".git")))) {
    await runner("git", ["clone", "--quiet", git, clone], { capture: true });
  }
  const has = await runner("git", ["cat-file", "-e", `${rev}^{commit}`], {
    cwd: clone,
    capture: true,
  }).then(
    () => true,
    () => false,
  );
  if (!has) await runner("git", ["fetch", "--quiet", "origin", rev], { cwd: clone, capture: true });
  await runner("git", ["-c", "advice.detachedHead=false", "checkout", "--quiet", "--force", rev], {
    cwd: clone,
    capture: true,
  });
}

async function exists(path: string): Promise<boolean> {
  return stat(path).then(
    () => true,
    () => false,
  );
}

/** Links `<build root>/hypha` to this checkout, so the apps' `../hypha/packages/*`
 * resolve to it. */
export async function linkHypha(dir: string, repo = REPO): Promise<void> {
  const root = buildRoot(dir);
  await mkdir(root, { recursive: true });
  const link = join(root, "hypha");
  const current = await readlink(link).catch(() => undefined);
  if (current === repo) return;
  await rm(link, { recursive: false, force: true });
  // A junction on Windows: no administrator rights needed.
  await symlink(repo, link, process.platform === "win32" ? "junction" : "dir");
}

/**
 * The app's clone at its commit, its dependencies installed: cloned the first time,
 * fetched when the commit is new to it, and left as it is when it's there already.
 */
export async function checkout(
  dir: string,
  a: AppEntry,
  runner: Runner = runHere,
): Promise<string> {
  if (!a.rev) throw new Error(`app ${a.name} isn't pinned: \`hypha apps update ${a.name}\``);
  // The app links this checkout's packages.
  requireRepo(`Building the ${a.name} app`);
  await linkHypha(dir);
  const clone = cloneDir(dir, a);
  await cloneAt(a.git, a.rev, clone, runner);
  // The SDK it links needs this checkout's own dependencies.
  if (!(await exists(join(REPO, "node_modules")))) {
    await runner("pnpm", ["install", "--frozen-lockfile"], { cwd: REPO, capture: true });
  }
  await runner("pnpm", ["install", "--frozen-lockfile"], { cwd: clone, capture: true });
  return clone;
}
