// Spores (design note 0030, §9): Rust crates compiled into the deployment's API. A
// first-party spore (this repository's `spores/<name>`, `"stripe"` in `hypha.json`) is a
// Cargo feature of the hosts (`spore-<name>`); any other is a crate from git, crates.io
// or a path, which needs host crates of the deployment's own, written next to this
// checkout (so Docker's build context holds both) and built instead of the stock ones:
// the native host (`writeHostCrate`: a server's API, and Cloudflare's `setup` tool) and
// the Worker (`writeWorkerCrate`).

import { mkdir, readFile, writeFile } from "node:fs/promises";
import { basename, join, relative, resolve, sep } from "node:path";

import { REPO } from "./templates.ts";

/** The spores this repository ships, each a feature of `hypha-host-axum` and
 * `hypha-host-workers` (`spore-<name>`). */
export const FIRST_PARTY_SPORES = ["stripe"] as const;

/** A spore in `hypha.json`: a first-party name alone (`"stripe"`), or a crate. */
export type SporeEntry =
  | string
  | {
      /** Its name (`NAME`): `spore_<name>` is its app's key. */
      name: string;
      /** The crate (`acme-hypha-reviews`), from one of `git` (with `rev`), `version`
       * (crates.io) or `path`. */
      crate: string;
      git?: string;
      rev?: string;
      version?: string;
      path?: string;
      /** Its `Spore` type (`acme_hypha_reviews::Reviews`). */
      spore: string;
      /** The route macro its `spore_routes!` defines (`acme_hypha_reviews::reviews_routes`);
       * absent for a spore without routes. */
      routes?: string;
    };

const NAME = /^[a-z][a-z0-9_]*$/;
const REV = /^[0-9a-f]{40}$/;
const RUST_PATH = /^[a-z_][a-z0-9_]*(::[A-Za-z_][A-Za-z0-9_]*)+$/;

/** The spore's name. */
export function sporeName(entry: SporeEntry): string {
  return typeof entry === "string" ? entry : entry.name;
}

/** Whether it's one of this repository's (a feature, no host crate). */
export function isFirstParty(entry: SporeEntry): entry is string {
  return typeof entry === "string";
}

/** `hypha.json`'s `spores`, checked: every problem a message naming `where`. The names
 * may not extend one another (`stripe`, `stripe_link`), as the hosts check. */
export function normalizeSpores(raw: unknown, where: string): SporeEntry[] {
  if (!Array.isArray(raw)) throw new Error(`${where}: \`spores\` is a list`);
  const out: SporeEntry[] = [];
  for (const item of raw) {
    let entry: SporeEntry;
    if (typeof item === "string") {
      if (!(FIRST_PARTY_SPORES as readonly string[]).includes(item))
        throw new Error(
          `${where}: ${JSON.stringify(item)} isn't a first-party spore (${FIRST_PARTY_SPORES.join(", ")}): give {name, crate, spore, and git+rev, version or path}`,
        );
      entry = item;
    } else if (item && typeof item === "object") {
      // oxlint-disable-next-line typescript/no-unsafe-type-assertion -- each field is checked next
      const o = item as Partial<Exclude<SporeEntry, string>>;
      const name = o.name;
      if (typeof name !== "string" || typeof o.crate !== "string" || typeof o.spore !== "string")
        throw new Error(`${where}: a spore needs \`name\`, \`crate\` and \`spore\``);
      const sources = [o.git, o.version, o.path].filter((s) => s !== undefined);
      if (sources.length !== 1)
        throw new Error(
          `${where}: spore ${name}: give one of \`git\` (with \`rev\`), \`version\` or \`path\``,
        );
      if (o.git !== undefined && (typeof o.rev !== "string" || !REV.test(o.rev)))
        throw new Error(`${where}: spore ${name}: \`rev\` is a full commit ID (40 hex digits)`);
      if (!RUST_PATH.test(o.spore))
        throw new Error(`${where}: spore ${name}: \`spore\` is a Rust path (crate_name::Type)`);
      if (o.routes !== undefined && !RUST_PATH.test(o.routes))
        throw new Error(
          `${where}: spore ${name}: \`routes\` is a Rust path (crate_name::name_routes)`,
        );
      entry = {
        name,
        crate: o.crate,
        spore: o.spore,
        ...(o.git !== undefined ? { git: o.git, rev: o.rev } : {}),
        ...(o.version !== undefined ? { version: o.version } : {}),
        ...(o.path !== undefined ? { path: o.path } : {}),
        ...(o.routes !== undefined ? { routes: o.routes } : {}),
      };
    } else {
      throw new Error(`${where}: a spore is a first-party name or {name, crate, spore, …}`);
    }
    const name = sporeName(entry);
    if (!NAME.test(name))
      throw new Error(
        `${where}: spore ${JSON.stringify(name)}: lowercase letters, digits and _, starting with a letter`,
      );
    for (const other of out.map(sporeName)) {
      if (other === name) throw new Error(`${where}: spore ${name} is listed twice`);
      const [shorter, longer] = other.length < name.length ? [other, name] : [name, other];
      if (longer.startsWith(`${shorter}_`))
        throw new Error(
          `${where}: spore ${longer}'s name extends ${shorter}'s: their tables and provider keys (spore_${shorter}_…) would overlap`,
        );
    }
    out.push(entry);
  }
  return out;
}

/** The hosts' features for a deployment's first-party spores (`spore-stripe`), when it
 * has no others; `undefined` when it has none at all. */
export function sporeFeatures(spores: SporeEntry[]): string | undefined {
  const first = spores.filter(isFirstParty);
  return first.length ? first.map((s) => `spore-${s}`).join(",") : undefined;
}

/** Whether the deployment needs a host crate of its own (a spore from outside this
 * repository). */
export function needsHostCrate(spores: SporeEntry[]): boolean {
  return spores.some((s) => !isFirstParty(s));
}

/** Where a deployment's host crate goes: in this checkout (Docker's build context),
 * git-ignored, named after the deployment's folder. */
export function hostCrateDir(deploymentDir: string): string {
  return join(REPO, ".hypha-build", basename(resolve(deploymentDir)), "host");
}

/** The host crate's `Cargo.toml`: the native host and every spore, its own workspace. */
export function hostCargoToml(dir: string, spores: SporeEntry[], deploymentDir = "."): string {
  const repo = (path: string) => JSON.stringify(relative(dir, join(REPO, path)));
  const lines = [
    "# Written by `hypha deploy` from hypha.json's `spores` (design note 0030, §9): change",
    "# those, not this file.",
    "[package]",
    'name = "hypha-deployment"',
    'version = "0.1.0"',
    'edition = "2024"',
    "publish = false",
    "",
    "# Its own workspace: not one of the checkout's crates.",
    "[workspace]",
    "",
    "[dependencies]",
    `hypha-host-axum = { path = ${repo("crates/hypha-host-axum")} }`,
  ];
  lines.push(...sporeDependencies(dir, spores, deploymentDir));
  lines.push("", "[profile.release]", 'debug = "line-tables-only"', "");
  return lines.join("\n");
}

/** A path spore's folder: resolved against the deployment's folder, and inside this
 * checkout, which is Docker's build context (one outside couldn't be built there). */
export function sporePath(name: string, deploymentDir: string, path: string): string {
  const full = resolve(deploymentDir, path);
  if (!full.startsWith(`${REPO}${sep}`))
    throw new Error(
      `spore ${name}: \`path\` ${full} is outside this checkout (${REPO}), which the API is built from: put the crate inside it, or give \`git\` or \`version\``,
    );
  return full;
}

/** The host crate's `main.rs`: `host! { spores: [...], routes: [...] }`. */
export function hostMain(spores: SporeEntry[]): string {
  const types = spores.map((s) => (isFirstParty(s) ? `hypha_spore_${s}::${pascal(s)}` : s.spore));
  const routes = spores.flatMap((s) =>
    isFirstParty(s) ? [`hypha_spore_${s}::${s}_routes`] : s.routes ? [s.routes] : [],
  );
  return [
    "//! Written by `hypha deploy` from hypha.json's `spores` (design note 0030, §9).",
    "",
    "hypha_host_axum::host! {",
    `    spores: [${types.join(", ")}],`,
    `    routes: [${routes.join(", ")}],`,
    "}",
    "",
  ].join("\n");
}

function pascal(name: string): string {
  return name
    .split("_")
    .map((w) => w.charAt(0).toUpperCase() + w.slice(1))
    .join("");
}

/** Where a deployment's Worker crate goes, beside its host crate. */
export function workerCrateDir(deploymentDir: string): string {
  return join(REPO, ".hypha-build", basename(resolve(deploymentDir)), "worker");
}

/** The spores' dependency lines for a crate in `dir`. */
function sporeDependencies(dir: string, spores: SporeEntry[], deploymentDir: string): string[] {
  return spores.map((s) => {
    if (isFirstParty(s))
      return `hypha-spore-${s} = { path = ${JSON.stringify(relative(dir, join(REPO, "spores", s)))} }`;
    const source = s.git
      ? `git = ${JSON.stringify(s.git)}, rev = ${JSON.stringify(s.rev)}`
      : s.version
        ? `version = ${JSON.stringify(s.version)}`
        : `path = ${JSON.stringify(relative(dir, sporePath(s.name, deploymentDir, s.path ?? ".")))}`;
    return `${s.crate} = { ${source} }`;
  });
}

/** The Worker crate's `Cargo.toml` (design note 0030, §9): `hypha-host-workers` without
 * its stock Worker, `worker` (its `#[event]`s expand to `::worker`), every spore. */
export function workerCargoToml(dir: string, spores: SporeEntry[], deploymentDir = "."): string {
  return [
    "# Written by `hypha deploy` from hypha.json's `spores` (design note 0030, §9): change",
    "# those, not this file.",
    "[package]",
    'name = "hypha-deployment-worker"',
    'version = "0.1.0"',
    'edition = "2024"',
    "publish = false",
    "",
    "[lib]",
    'crate-type = ["cdylib"]',
    "",
    "# Its own workspace: not one of the checkout's crates.",
    "[workspace]",
    "",
    "[dependencies]",
    `hypha-host-workers = { path = ${JSON.stringify(relative(dir, join(REPO, "crates", "hypha-host-workers")))}, default-features = false }`,
    `worker = { version = "=0.8.7", features = ["http", "axum", "tokio-postgres"] }`,
    ...sporeDependencies(dir, spores, deploymentDir),
    "",
  ].join("\n");
}

/** The Worker crate's `lib.rs`: `worker! { spores: [...], routes: [...] }`. */
export function workerLib(spores: SporeEntry[]): string {
  return hostMain(spores)
    .replace("hypha_host_axum::host!", "hypha_host_workers::worker!")
    .replace("`hypha deploy`", "`hypha deploy` (the Worker)");
}

/** Writes the deployment's Worker crate (its lock seeded like the host crate's) and
 * returns its folder (absolute: wrangler builds there). */
export async function writeWorkerCrate(
  deploymentDir: string,
  spores: SporeEntry[],
): Promise<string> {
  const dir = workerCrateDir(deploymentDir);
  await mkdir(join(dir, "src"), { recursive: true });
  await writeFile(join(dir, "Cargo.toml"), workerCargoToml(dir, spores, deploymentDir));
  await writeFile(join(dir, "src", "lib.rs"), workerLib(spores));
  const lock = join(dir, "Cargo.lock");
  const existing = await readFile(lock, "utf8").catch(() => undefined);
  if (existing === undefined)
    await writeFile(lock, await readFile(join(REPO, "Cargo.lock"), "utf8"));
  return dir;
}

/** Writes the deployment's host crate (seeding its `Cargo.lock` from the checkout's, so
 * Hypha's own dependencies resolve as they're tested), and returns its folder relative
 * to the checkout (what the API's Dockerfile builds: `HOST_CRATE`). */
export async function writeHostCrate(deploymentDir: string, spores: SporeEntry[]): Promise<string> {
  const dir = hostCrateDir(deploymentDir);
  await mkdir(join(dir, "src"), { recursive: true });
  await writeFile(join(dir, "Cargo.toml"), hostCargoToml(dir, spores, deploymentDir));
  await writeFile(join(dir, "src", "main.rs"), hostMain(spores));
  const lock = join(dir, "Cargo.lock");
  const existing = await readFile(lock, "utf8").catch(() => undefined);
  if (existing === undefined)
    await writeFile(lock, await readFile(join(REPO, "Cargo.lock"), "utf8"));
  return relative(REPO, dir);
}
