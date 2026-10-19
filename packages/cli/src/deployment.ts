// A deployment is a folder `hypha init` writes: `hypha.json` (what the CLI decided:
// where it runs, the domains, the apps) and the files the server runs (`compose.yaml`,
// `Caddyfile`, `.env` with the secrets, `hypha.config.toml`, `seaweedfs/s3.json`). The
// folder is the source of truth: `hypha deploy` copies it to the server each time.

import { chmod, readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";

import { normalizeApps } from "./apps.ts";

export const STATE_FILE = "hypha.json";
export const ENV_FILE = ".env";

/** Where the stack runs: a server reached over SSH, this machine (to try it out), or
 * Cloudflare's Workers (the API over Hyperdrive to any Postgres; design note 0026). */
export type Target = "vps" | "local" | "cloudflare";

/** Where the images are built: on the server itself, or here and then copied to it. */
export type BuildOn = "server" | "here";

export type App = "admin" | "storefront";

/** A Cloudflare deployment's account and what `hypha deploy` made there. */
export interface Cloudflare {
  /** Names its resources: the Workers `<name>-api`, `<name>-admin`, `<name>-storefront`,
   * the bucket `<name>-media`, the KV namespaces `<name>-jobs` and `<name>-cache`,
   * Hyperdrive `<name>`. Fixed once deployed. */
  name: string;
  /** The account (from `wrangler whoami` when it reaches only one). */
  accountId?: string;
  /** The media domain's zone, for the bucket's custom domain; without it, signed URLs. */
  zoneId?: string;
  /** A placement region (`aws:eu-central-1`) instead of measuring the database's host. */
  region?: string;
  /** The first rate limiter's `namespace_id`; the rules take the next ones. Unique per
   * account, so two deployments on one account don't share counts. */
  rateLimitBase: number;
  hyperdriveId?: string;
  kvId?: string;
  /** The storefront's cache (`<name>-cache`): ISR data shared by its isolates. */
  cacheKvId?: string;
}

export interface Deployment {
  version: 1;
  target: Target;
  /** The SSH destination (`root@203.0.113.7`), for a server. */
  ssh?: string;
  /** The deployment's folder on the server. */
  remoteDir: string;
  buildOn: BuildOn;
  /** `https` on a server (Caddy gets certificates); `http` on this machine. */
  scheme: "https" | "http";
  domains: {
    api: string;
    media: string;
    admin?: string;
    storefront?: string;
  };
  /** With `target: "cloudflare"`. */
  cloudflare?: Cloudflare;
  /** The domain the others go under (`example.com`): co-located apps default to
   * `<name>.<domain>`. Older deployments lack it (taken from the API's then). */
  domain?: string;
  /** For Let's Encrypt's notices: `hypha deploy` writes the apps' Caddy sites with it. */
  acmeEmail?: string;
  /** Apps deployed alongside (design note 0029, §9): each built from its repository at
   * a pinned commit, `hypha apps add|update` (apps.ts). In the file, a first-party
   * app's name alone (`"stripe"`) stands for its repository, unpinned until a deploy. */
  apps?: AppEntry[];
}

/** An app deployed with the deployment (design note 0029, §9). */
export interface AppEntry {
  /** Names its Worker (`<name>-app-<name>`), container (`app-<name>`), domain and
   * encryption key (`APP_<NAME>_ENCRYPTION_KEY` in `.env`). */
  name: string;
  /** Its repository, cloned with this machine's git credentials. */
  git: string;
  /** The commit it's built from (`hypha apps update` moves it). */
  rev?: string;
  /** Its domain; `<name>.<domain>` by default. */
  domain?: string;
  /** On Cloudflare: its D1 database, made by the first deploy. */
  d1Id?: string;
  /** On Cloudflare: its Worker exists, so the API may bind to it. */
  deployed?: boolean;
}

export function apps(d: Deployment): App[] {
  return (["admin", "storefront"] as const).filter((app) => d.domains[app]);
}

export function url(d: Deployment, host: string): string {
  return `${d.scheme}://${host}`;
}

export async function loadDeployment(dir: string): Promise<Deployment> {
  let text: string;
  try {
    text = await readFile(join(dir, STATE_FILE), "utf8");
  } catch {
    throw new Error(`no deployment in ${dir}: run \`hypha init\` first (or pass --dir)`);
  }
  // oxlint-disable-next-line typescript/no-unsafe-type-assertion -- written by `saveDeployment`; its version is checked next
  const d = JSON.parse(text) as Deployment;
  if (d.version !== 1) {
    throw new Error(
      `${join(dir, STATE_FILE)}: version ${JSON.stringify(d.version)} is newer than this CLI`,
    );
  }
  if (d.apps !== undefined) d.apps = normalizeApps(d.apps, join(dir, STATE_FILE));
  return d;
}

export async function saveDeployment(dir: string, d: Deployment): Promise<void> {
  await writeFile(join(dir, STATE_FILE), `${JSON.stringify(d, null, 2)}\n`);
}

// ---------- .env ----------

/** One line of a `.env` file: a variable, or a comment or blank kept as written. */
type Line = { key: string; value: string } | { text: string };

function isBlank(line: Line | undefined): boolean {
  return line !== undefined && "text" in line && line.text === "";
}

/** A `.env` file as Docker Compose reads it, kept in order with its comments. */
export class EnvFile {
  private lines: Line[];

  constructor(lines: Line[] = []) {
    this.lines = lines;
  }

  static parse(text: string): EnvFile {
    const lines: Line[] = [];
    for (const raw of text.split("\n")) {
      const match = /^([A-Za-z_][A-Za-z0-9_]*)=(.*)$/.exec(raw);
      if (!match) {
        lines.push({ text: raw });
        continue;
      }
      let value = match[2];
      if (value.startsWith('"') && value.endsWith('"') && value.length >= 2) {
        value = value.slice(1, -1).replace(/\\(["\\])/g, "$1");
      } else if (value.startsWith("'") && value.endsWith("'") && value.length >= 2) {
        value = value.slice(1, -1);
      }
      lines.push({ key: match[1], value });
    }
    while (isBlank(lines.at(-1))) lines.pop();
    return new EnvFile(lines);
  }

  /** The variables' names, in order. */
  keys(): string[] {
    return this.lines.flatMap((l) => ("key" in l ? [l.key] : []));
  }

  get(key: string): string | undefined {
    for (const line of this.lines) if ("key" in line && line.key === key) return line.value;
    return undefined;
  }

  /** Sets `key`, in place if it's there, else at the end. */
  set(key: string, value: string): this {
    const line = this.lines.find((l) => "key" in l && l.key === key);
    if (line && "key" in line) line.value = value;
    else this.lines.push({ key, value });
    return this;
  }

  comment(text: string): this {
    this.lines.push(...text.split("\n").map((t) => ({ text: t ? `# ${t}` : "" })));
    return this;
  }

  toString(): string {
    return `${this.lines.map((l) => ("key" in l ? `${l.key}=${formatValue(l.value)}` : l.text)).join("\n")}\n`;
  }
}

/** Plain when it's safe, else double-quoted (Compose unescapes `\"` and `\\`). */
function formatValue(value: string): string {
  return /^[A-Za-z0-9_./:@,+-]*$/.test(value) ? value : `"${value.replace(/(["\\])/g, "\\$1")}"`;
}

export async function loadEnv(dir: string): Promise<EnvFile> {
  return EnvFile.parse(await readFile(join(dir, ENV_FILE), "utf8"));
}

/** Writes `.env` readable by its owner only: it holds the deployment's secrets. */
export async function saveEnv(dir: string, env: EnvFile): Promise<void> {
  const path = join(dir, ENV_FILE);
  await writeFile(path, env.toString(), { mode: 0o600 });
  await chmod(path, 0o600);
}
