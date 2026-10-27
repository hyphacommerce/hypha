// Packs the publishable packages as a release would (`pnpm packages:build` first), for
// checks that install them into a temporary project: `check-packages.mjs` and
// `check-starters.mjs`.

import { cpSync, readFileSync, readdirSync, rmSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

export const repo = fileURLToPath(new URL("../..", import.meta.url));
const packagesDir = join(repo, "packages");
const LICENSES = ["LICENSE-MIT", "LICENSE-APACHE"];

/** The publishable packages (not `private`) with their manifests. */
export function publishablePackages() {
  return readdirSync(packagesDir)
    .map((dir) => ({ dir: join(packagesDir, dir), manifest: null }))
    .map((p) => ({ ...p, manifest: JSON.parse(readFileSync(join(p.dir, "package.json"), "utf8")) }))
    .filter((p) => !p.manifest.private);
}

/**
 * Packs each package into `into`; answers the tarballs by package name as `file:` specs.
 * The licence files travel with each package (listed in its `files`): call
 * `cleanLicenses` when done.
 */
export function packPackages(into) {
  const tarballs = new Map();
  for (const p of publishablePackages()) {
    for (const name of LICENSES) cpSync(join(repo, name), join(p.dir, name));
    const out = execFileSync("pnpm", ["pack", "--pack-destination", into, "--json"], {
      cwd: p.dir,
      stdio: ["ignore", "pipe", "inherit"],
      encoding: "utf8",
    });
    const { filename } = JSON.parse(out.slice(out.indexOf("{")));
    tarballs.set(p.manifest.name, `file:${filename}`);
  }
  return tarballs;
}

export function cleanLicenses() {
  for (const dir of readdirSync(packagesDir))
    for (const name of LICENSES) rmSync(join(packagesDir, dir, name), { force: true });
}

/** pnpm's `overrides` pointing the packages' dependencies on each other at the tarballs. */
export function overridesYaml(tarballs) {
  return (
    "overrides:\n" +
    [...tarballs]
      .map(([name, spec]) => `  ${JSON.stringify(name)}: ${JSON.stringify(spec)}\n`)
      .join("")
  );
}
