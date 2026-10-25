// Spores: Rust crates compiled into the deployment's API, each from git (the first-party
// ones from their own repositories: `"stripe"` in `hypha.json` stands for
// hyphacommerce/spore-stripe), crates.io or a path. A deployment with spores gets host
// crates of its own, written into this checkout (so Docker's build context holds them):
// the native host (`writeHostCrate`: a server's API, and Cloudflare's `setup` tool) and
// the Worker (`writeWorkerCrate`). A git spore is cloned here at its commit with this
// machine's git credentials (`fetchSpores`) and built from that clone, so the build
// needs none.

import { createHash } from "node:crypto";
import { readdirSync } from "node:fs";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { basename, join, relative, resolve, sep } from "node:path";

import { type Runner, cloneAt } from "./apps.ts";
import { REPO, requireRepo } from "./templates.ts";

/** Hypha's repository, as spores outside it name it in their git dependencies. */
export const HYPHA_GIT = "https://github.com/hyphacommerce/hypha";

/** A spore in `hypha.json`, after a first-party name alone (`"stripe"`) is filled in. */
export type SporeEntry = {
  /** Its name (`NAME`): `spore_<name>` is its app's key. */
  name: string;
  /** The crate (`acme-hypha-reviews`), from one of `git` (with `rev`, set by the next
   * deploy when absent), `version` (crates.io) or `path`. */
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

/** The first-party spores, by name: each in its own repository. */
export const FIRST_PARTY_SPORES: Record<string, Omit<SporeEntry, "name">> = {
  stripe: {
    crate: "hypha-spore-stripe",
    git: "git@github.com:hyphacommerce/spore-stripe.git",
    spore: "hypha_spore_stripe::Stripe",
    routes: "hypha_spore_stripe::stripe_routes",
  },
};

const NAME = /^[a-z][a-z0-9_]*$/;
const REV = /^[0-9a-f]{40}$/;
const RUST_PATH = /^[a-z_][a-z0-9_]*(::[A-Za-z_][A-Za-z0-9_]*)+$/;

/** The spore's name. */
export function sporeName(entry: SporeEntry): string {
  return entry.name;
}

/** `hypha.json`'s `spores`, checked: every problem a message naming `where`. A
 * first-party name alone is filled in from its repository. The names may not extend one
 * another (`stripe`, `stripe_link`), as the hosts check. */
export function normalizeSpores(raw: unknown, where: string): SporeEntry[] {
  if (!Array.isArray(raw)) throw new Error(`${where}: \`spores\` is a list`);
  const out: SporeEntry[] = [];
  const known = Object.keys(FIRST_PARTY_SPORES).join(", ");
  for (const item of raw) {
    let o: Partial<SporeEntry>;
    if (typeof item === "string") {
      const first = FIRST_PARTY_SPORES[item];
      if (!first)
        throw new Error(
          `${where}: ${JSON.stringify(item)} isn't a first-party spore (${known}): give {name, crate, spore, and git, version or path}`,
        );
      o = { name: item, ...first };
    } else if (item && typeof item === "object") {
      // oxlint-disable-next-line typescript/no-unsafe-type-assertion -- each field is checked next
      const given = item as Partial<SporeEntry>;
      const first = given.name ? FIRST_PARTY_SPORES[given.name] : undefined;
      // A first-party spore's entry carries its pin; the rest comes from here.
      o = first && given.crate === undefined ? { ...first, ...given } : given;
    } else {
      throw new Error(`${where}: a spore is a first-party name or {name, crate, spore, …}`);
    }
    const name = o.name;
    if (typeof name !== "string" || typeof o.crate !== "string" || typeof o.spore !== "string")
      throw new Error(`${where}: a spore needs \`name\`, \`crate\` and \`spore\``);
    const sources = [o.git, o.version, o.path].filter((s) => s !== undefined);
    if (sources.length !== 1)
      throw new Error(`${where}: spore ${name}: give one of \`git\`, \`version\` or \`path\``);
    if (o.rev !== undefined && (o.git === undefined || !REV.test(o.rev)))
      throw new Error(
        `${where}: spore ${name}: \`rev\` is a full commit ID (40 hex digits), with \`git\``,
      );
    if (!RUST_PATH.test(o.spore))
      throw new Error(`${where}: spore ${name}: \`spore\` is a Rust path (crate_name::Type)`);
    if (o.routes !== undefined && !RUST_PATH.test(o.routes))
      throw new Error(
        `${where}: spore ${name}: \`routes\` is a Rust path (crate_name::name_routes)`,
      );
    const entry: SporeEntry = {
      name,
      crate: o.crate,
      spore: o.spore,
      ...(o.git !== undefined ? { git: o.git } : {}),
      ...(o.rev !== undefined ? { rev: o.rev } : {}),
      ...(o.version !== undefined ? { version: o.version } : {}),
      ...(o.path !== undefined ? { path: o.path } : {}),
      ...(o.routes !== undefined ? { routes: o.routes } : {}),
    };
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

/** The deployment's generated crates, in this checkout: git-ignored, named after the
 * deployment's folder. */
export function buildDir(deploymentDir: string): string {
  return join(
    requireRepo("A deployment with spores"),
    ".hypha-build",
    basename(resolve(deploymentDir)),
  );
}

/** Where a deployment's host crate goes. */
export function hostCrateDir(deploymentDir: string): string {
  return join(buildDir(deploymentDir), "host");
}

/** Where a git spore is cloned, beside the host crate (in Docker's build context). */
export function sporeCloneDir(deploymentDir: string, name: string): string {
  return join(buildDir(deploymentDir), "spores", name);
}

/** Each git spore cloned at its commit: its name → the clone. A spore not pinned yet is
 * refused (`hypha deploy` pins them first). */
export async function fetchSpores(
  deploymentDir: string,
  spores: SporeEntry[],
  runner?: Runner,
): Promise<Record<string, string>> {
  const local: Record<string, string> = {};
  for (const s of spores) {
    if (!s.git) continue;
    if (!s.rev) throw new Error(`spore ${s.name} isn't pinned to a commit (\`rev\`)`);
    const clone = sporeCloneDir(deploymentDir, s.name);
    await cloneAt(s.git, s.rev, clone, runner);
    local[s.name] = clone;
  }
  return local;
}

/** The host crate's `Cargo.toml`: the native host and every spore, its own workspace.
 * `local` is where each git spore's crate is on this machine (`fetchSpores`). */
export function hostCargoToml(
  dir: string,
  spores: SporeEntry[],
  deploymentDir = ".",
  local: Record<string, string> = {},
): string {
  const repo = (path: string) => JSON.stringify(relative(dir, join(REPO, path)));
  const lines = [
    "# Written by `hypha deploy` from hypha.json's `spores`: change",
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
  lines.push(...sporeDependencies(dir, spores, deploymentDir, local));
  lines.push(...hyphaPatch(dir));
  lines.push("", "[profile.release]", 'debug = "line-tables-only"', "");
  return lines.join("\n");
}

/** Spores depend on Hypha through git: this points that source at this checkout, so the
 * spores and the host share one copy of each crate (two would be two `Spore` traits), and
 * the build fetches nothing. Cargo warns about the crates no spore uses. */
function hyphaPatch(dir: string): string[] {
  const crates = readdirSync(join(REPO, "crates"), { withFileTypes: true })
    .filter((e) => e.isDirectory())
    .map((e) => e.name)
    .toSorted();
  return [
    "",
    `[patch.${JSON.stringify(HYPHA_GIT)}]`,
    ...crates.map(
      (c) => `${c} = { path = ${JSON.stringify(relative(dir, join(REPO, "crates", c)))} }`,
    ),
  ];
}

/** A path spore's folder: resolved against the deployment's folder, and inside this
 * checkout's `spores/`, the one folder of it besides the crates and `.hypha-build/` that
 * goes into Docker's build context (`docker/api.Dockerfile.dockerignore`). */
export function sporePath(name: string, deploymentDir: string, path: string): string {
  const full = resolve(deploymentDir, path);
  const spores = join(REPO, "spores");
  if (!full.startsWith(`${spores}${sep}`))
    throw new Error(
      `spore ${name}: \`path\` ${full} is outside ${spores}, which the API is built from: put the crate there, or give \`git\` or \`version\``,
    );
  return full;
}

/** The host crate's `main.rs`: `host! { spores: [...], routes: [...] }`. */
export function hostMain(spores: SporeEntry[]): string {
  return crateEntry("hypha_host_axum::host!", "`hypha deploy`", spores);
}

/** A generated crate's entry: `<macro> { spores: [...], routes: [...] }`. */
function crateEntry(macro: string, writer: string, spores: SporeEntry[]): string {
  const types = spores.map((s) => s.spore);
  const routes = spores.flatMap((s) => (s.routes ? [s.routes] : []));
  return [
    `//! Written by ${writer} from hypha.json's \`spores\`.`,
    "",
    `${macro} {`,
    `    spores: [${types.join(", ")}],`,
    `    routes: [${routes.join(", ")}],`,
    "}",
    "",
  ].join("\n");
}

/** Where a deployment's Worker crate goes, beside its host crate. */
export function workerCrateDir(deploymentDir: string): string {
  return join(buildDir(deploymentDir), "worker");
}

/** The spores' dependency lines for a crate in `dir`. */
function sporeDependencies(
  dir: string,
  spores: SporeEntry[],
  deploymentDir: string,
  local: Record<string, string>,
): string[] {
  return spores.map((s) => {
    let source: string;
    if (s.git) {
      const clone = local[s.name];
      if (!clone) throw new Error(`spore ${s.name} hasn't been fetched (\`fetchSpores\`)`);
      source = `path = ${JSON.stringify(relative(dir, clone))}`;
    } else if (s.version) {
      source = `version = ${JSON.stringify(s.version)}`;
    } else {
      source = `path = ${JSON.stringify(relative(dir, sporePath(s.name, deploymentDir, s.path ?? ".")))}`;
    }
    return `${s.crate} = { ${source} }`;
  });
}

/** The Worker crate's `Cargo.toml`: `hypha-host-workers` without its stock Worker, `worker`
 * (its `#[event]`s expand to `::worker`), every spore. */
export function workerCargoToml(
  dir: string,
  spores: SporeEntry[],
  worker: string,
  deploymentDir = ".",
  local: Record<string, string> = {},
): string {
  return [
    "# Written by `hypha deploy` from hypha.json's `spores`: change",
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
    worker,
    ...sporeDependencies(dir, spores, deploymentDir, local),
    ...hyphaPatch(dir),
    "",
  ].join("\n");
}

/** The Worker crate's `lib.rs`: `worker! { spores: [...], routes: [...] }`. */
export function workerLib(spores: SporeEntry[]): string {
  return crateEntry("hypha_host_workers::worker!", "`hypha deploy` (the Worker)", spores);
}

/** `hypha-host-workers`' own `worker = …` line: the Worker crate needs the very same
 * `worker` (its `#[event]`s expand to `::worker`), pinned exactly as the host is. */
export async function workerDependency(): Promise<string> {
  const toml = await readFile(join(REPO, "crates", "hypha-host-workers", "Cargo.toml"), "utf8");
  const line = toml.split("\n").find((l) => /^worker\s*=/.test(l));
  if (!line) throw new Error("crates/hypha-host-workers/Cargo.toml names no `worker` dependency");
  return line.trim();
}

/** Writes the deployment's Worker crate and returns its folder (absolute: wrangler
 * builds there). */
export async function writeWorkerCrate(
  deploymentDir: string,
  spores: SporeEntry[],
  local: Record<string, string> = {},
): Promise<string> {
  const dir = workerCrateDir(deploymentDir);
  const toml = workerCargoToml(dir, spores, await workerDependency(), deploymentDir, local);
  await writeCrate(dir, { "Cargo.toml": toml, "src/lib.rs": workerLib(spores) });
  return dir;
}

/** Writes the deployment's host crate and returns its folder relative to the checkout
 * (what the API's Dockerfile builds: `HOST_CRATE`). */
export async function writeHostCrate(
  deploymentDir: string,
  spores: SporeEntry[],
  local: Record<string, string> = {},
): Promise<string> {
  const dir = hostCrateDir(deploymentDir);
  const toml = hostCargoToml(dir, spores, deploymentDir, local);
  await writeCrate(dir, { "Cargo.toml": toml, "src/main.rs": hostMain(spores) });
  return relative(REPO, dir);
}

/** Writes a generated crate's files, and its `Cargo.lock` from the checkout's, so
 * Hypha's own dependencies resolve as they're tested. The lock is written again only
 * when the checkout's changed since (`Cargo.lock.seed` holds its hash): until then the
 * crate keeps its own, with what Cargo resolved for the spores. */
async function writeCrate(dir: string, files: Record<string, string>): Promise<void> {
  await mkdir(join(dir, "src"), { recursive: true });
  for (const [path, text] of Object.entries(files)) await writeFile(join(dir, path), text);
  const checkout = await readFile(join(REPO, "Cargo.lock"), "utf8");
  const hash = createHash("sha256").update(checkout).digest("hex");
  const seed = join(dir, "Cargo.lock.seed");
  if ((await readFile(seed, "utf8").catch(() => undefined)) === hash) return;
  await writeFile(join(dir, "Cargo.lock"), checkout);
  await writeFile(seed, hash);
}
