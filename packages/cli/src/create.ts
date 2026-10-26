// `hypha create <starter> <dir>`: a project of your own from one of Hypha's starters, the
// storefront or an app, at the version of this CLI. The starter is a folder of this
// repository: copied from the checkout when the CLI runs from one, else downloaded from the
// release's tag (one tag releases the CLI, the packages and the starters together, so they
// match). Its `workspace:*` dependencies become that version, published on npm, and it
// gets what the workspace gave it (the package manager, a `.gitignore`, pnpm's settings)
// so it installs and builds on its own.

import { cp, mkdir, mkdtemp, readFile, readdir, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, join, resolve } from "node:path";

import { run } from "./shell.ts";
import { REPO, inCheckout } from "./templates.ts";
import { VERSION } from "./version.ts";

/** The repository the starters are downloaded from. */
export const REPOSITORY = "hyphacommerce/hypha";

export interface Starter {
  /** The folder in the repository. */
  path: string;
  title: string;
  /** Printed after creating it. */
  next: string[];
}

export const STARTERS = {
  storefront: {
    path: "apps/storefront",
    title: "Hypha's storefront: a server-rendered shop on Next.js, vinext and Astryx",
    next: [
      "cp .env.example .dev.vars   # then fill in HYPHA_API_URL and a publishable key",
      "pnpm install",
      "pnpm dev                    # http://localhost:3002",
    ],
  },
  app: {
    path: "starters/app",
    title:
      "An app for Hypha's apps platform: installed over OAuth, answering webhooks and an extension point",
    next: [
      "pnpm install",
      "pnpm test",
      "APP_URL=http://localhost:8787 pnpm dev   # then install it from your admin: Apps → Install app",
    ],
  },
} as const satisfies Record<string, Starter>;

export type StarterName = keyof typeof STARTERS;

export function isStarter(name: string): name is StarterName {
  return Object.hasOwn(STARTERS, name);
}

/** What a copy leaves behind: a workspace's builds, caches and local secrets. */
const SKIPPED = new Set([
  "node_modules",
  "dist",
  ".next",
  ".vinext",
  ".cloudflare",
  ".wrangler",
  ".dev.vars",
  "next-env.d.ts",
  ".DS_Store",
]);

function skipped(name: string): boolean {
  return SKIPPED.has(name) || name.endsWith(".tsbuildinfo") || /^\.env.*\.local$/.test(name);
}

/** The tag a version was released as. */
export function tagOf(version: string): string {
  return `v${version}`;
}

/** The release's source archive, and the folder inside it. */
export function archiveOf(ref: string): { url: string; root: string } {
  const tag = ref.startsWith("v") || !/^\d/.test(ref) ? ref : tagOf(ref);
  return {
    url: `https://codeload.github.com/${REPOSITORY}/tar.gz/${encodeURIComponent(tag)}`,
    // GitHub names the archive's folder `<repo>-<ref>`: the ref without its `v`, each `/` a `-`.
    root: `hypha-${tag.replace(/^v/, "").replaceAll("/", "-")}`,
  };
}

/** The starter's `.gitignore`: what the workspace's covered for it. */
export const GITIGNORE = `node_modules/
dist/
.next/
.cloudflare/
.wrangler/
.vinext/
*.tsbuildinfo
.env*.local
.dev.vars
wrangler.toml
next-env.d.ts
.DS_Store
`;

/** pnpm's settings for the project (pnpm reads them from this file, not package.json). */
export function workspaceYaml(): string {
  return (
    "# pnpm's settings for this project.\n" +
    "# Hypha's packages are released together with this starter: never hold them back as too new.\n" +
    "minimumReleaseAgeExclude:\n" +
    '  - "@hyphacommerce/*"\n' +
    "# Dependency install scripts run only when listed here (pnpm refuses an install with one\n" +
    "# it wasn't told about). workerd is the Workers runtime the dev server runs in; esbuild and\n" +
    "# sharp find their binaries without their scripts; Astryx's script only prints a hint.\n" +
    "allowBuilds:\n" +
    "  workerd: true\n" +
    "  esbuild: false\n" +
    "  sharp: false\n" +
    '  "@astryxdesign/core": false\n' +
    '  "@astryxdesign/cli": false\n'
  );
}

type Manifest = Record<string, unknown> & {
  dependencies?: Record<string, string>;
  devDependencies?: Record<string, string>;
};

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** A record of strings from a JSON value, or none. */
function strings(value: unknown): Record<string, string> | undefined {
  if (!isRecord(value)) return undefined;
  const out: Record<string, string> = {};
  for (const [key, item] of Object.entries(value)) if (typeof item === "string") out[key] = item;
  return out;
}

/** A JSON object from `text`; anything else is refused with `what`. */
function parseObject(text: string, what: string): Record<string, unknown> {
  const value: unknown = JSON.parse(text);
  if (!isRecord(value)) throw new Error(`${what} isn't a JSON object`);
  return value;
}

/**
 * The project's `package.json` from the starter's: named after its folder, its
 * `workspace:*` dependencies at `version` (Hypha's packages on npm), and the workspace's
 * `packageManager` and `engines`, which pnpm and Node read from the project.
 */
export function adaptManifest(
  source: Manifest,
  name: string,
  version: string,
  root: { packageManager?: string; engines?: Record<string, string> },
): Manifest {
  const swap = (deps: Record<string, string> | undefined) =>
    deps &&
    Object.fromEntries(
      Object.entries(deps).map(([dep, spec]) => [
        dep,
        spec.startsWith("workspace:") ? version : spec,
      ]),
    );
  const { version: _version, ...rest } = source;
  return {
    ...rest,
    name,
    private: true,
    ...(root.packageManager ? { packageManager: root.packageManager } : {}),
    ...(root.engines ? { engines: root.engines } : {}),
    dependencies: swap(source.dependencies),
    devDependencies: swap(source.devDependencies),
  };
}

/** A project name from the folder: lowercase, what npm allows. */
export function projectName(dir: string): string {
  const name = basename(resolve(dir))
    .toLowerCase()
    .replaceAll(/[^a-z0-9._-]+/g, "-")
    .replace(/^[-._]+/, "");
  return name || "hypha-project";
}

/** Copies `from` into `to`, leaving builds, caches and local secrets behind. */
async function copyStarter(from: string, to: string): Promise<void> {
  await cp(from, to, {
    recursive: true,
    filter: (path) => !skipped(basename(path)),
  });
}

/** Downloads the release's archive and extracts the starter's folder from it. */
async function downloadStarter(ref: string, path: string, to: string): Promise<void> {
  const { url, root } = archiveOf(ref);
  const work = await mkdtemp(join(tmpdir(), "hypha-create-"));
  try {
    const response = await fetch(url);
    if (!response.ok)
      throw new Error(
        response.status === 404
          ? `no release ${ref} of ${REPOSITORY} (${url}): pass --ref to create from another tag or branch`
          : `downloading ${url}: HTTP ${response.status}`,
      );
    const archive = join(work, "source.tar.gz");
    await writeFile(archive, new Uint8Array(await response.arrayBuffer()));
    const depth = path.split("/").length + 1;
    // The system's tar: present on macOS, Linux and Windows 10+.
    await run(
      "tar",
      ["-xzf", archive, "-C", to, `--strip-components=${depth}`, `${root}/${path}`],
      { capture: true },
    );
  } finally {
    await rm(work, { recursive: true, force: true });
  }
}

export interface CreateOptions {
  /** A tag or branch to take the starter from instead of this CLI's release. */
  ref?: string;
  /** Copy from this checkout of Hypha instead of downloading (tests, `pnpm hypha` here). */
  from?: string;
}

export interface Created {
  dir: string;
  name: string;
  /** Where the files came from: the release's tag, a ref, or a checkout. */
  source: string;
}

/** Makes `dir` from the starter `kind`; refuses a folder that isn't empty. */
export async function create(
  kind: StarterName,
  dir: string,
  options: CreateOptions = {},
): Promise<Created> {
  const starter = STARTERS[kind];
  const target = resolve(dir);
  if (await exists(target)) {
    if ((await readdir(target)).length > 0) throw new Error(`${dir} exists and isn't empty`);
  }
  const checkout = options.from ?? (options.ref === undefined && inCheckout() ? REPO : undefined);
  let source: string;
  let root: { packageManager?: string; engines?: Record<string, string> } = {};
  if (checkout) {
    await copyStarter(join(checkout, starter.path), target);
    root = pick(
      parseObject(await readFile(join(checkout, "package.json"), "utf8"), "package.json"),
    );
    source = `the checkout at ${checkout}`;
  } else {
    const ref = options.ref ?? tagOf(VERSION);
    await mkdir(target, { recursive: true });
    await downloadStarter(ref, starter.path, target);
    root = { packageManager: ROOT_PACKAGE_MANAGER, engines: ROOT_ENGINES };
    source = `${REPOSITORY} at ${ref}`;
  }
  const name = projectName(target);
  const manifestPath = join(target, "package.json");
  const raw = parseObject(await readFile(manifestPath, "utf8"), `${starter.path}/package.json`);
  const manifest: Manifest = {
    ...raw,
    dependencies: strings(raw.dependencies),
    devDependencies: strings(raw.devDependencies),
  };
  await writeFile(
    manifestPath,
    JSON.stringify(adaptManifest(manifest, name, VERSION, root), null, 2) + "\n",
  );
  await writeFile(join(target, ".gitignore"), GITIGNORE);
  await writeFile(join(target, "pnpm-workspace.yaml"), workspaceYaml());
  return { dir: target, name, source };
}

/** What the workspace's root package.json pins, read when creating from a checkout. */
function pick(root: Record<string, unknown>): {
  packageManager?: string;
  engines?: Record<string, string>;
} {
  const engines = strings(root.engines);
  return {
    ...(typeof root.packageManager === "string" ? { packageManager: root.packageManager } : {}),
    ...(engines ? { engines } : {}),
  };
}

// What the workspace pins, for a project made from a download (the root package.json isn't
// in the starter's folder). Kept in step with the root package.json by a test.
export const ROOT_PACKAGE_MANAGER = "pnpm@12.6.0";
export const ROOT_ENGINES = { node: ">=24" };

async function exists(path: string): Promise<boolean> {
  return stat(path).then(
    () => true,
    () => false,
  );
}
