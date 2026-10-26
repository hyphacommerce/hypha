// Checks the starters work outside the workspace, as `hypha create` hands them out: each
// is created by the CLI from this checkout into a temporary folder, its `@hyphacommerce/*`
// dependencies pointed at the packages packed from this commit (`pnpm packages:build`
// first), installed, type-checked, tested and built. Run by CI; `pnpm starters:check`
// locally. `STARTERS=app` checks only those named.

import { execFileSync } from "node:child_process";
import {
  appendFileSync,
  cpSync,
  existsSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const repo = fileURLToPath(new URL("..", import.meta.url));
const packagesDir = join(repo, "packages");
const work = mkdtempSync(join(tmpdir(), "hypha-starters-"));
const run = (cmd, args, cwd = work) =>
  execFileSync(cmd, args, { cwd, stdio: ["ignore", "pipe", "inherit"], encoding: "utf8" });
const only = process.env.STARTERS?.split(",")
  .map((s) => s.trim())
  .filter(Boolean);

try {
  // The published packages, packed as `pnpm packages:check` packs them.
  const tarballs = new Map();
  for (const dir of readdirSync(packagesDir)) {
    const manifest = JSON.parse(readFileSync(join(packagesDir, dir, "package.json"), "utf8"));
    if (manifest.private) continue;
    for (const name of ["LICENSE-MIT", "LICENSE-APACHE"])
      cpSync(join(repo, name), join(packagesDir, dir, name));
    const out = run("pnpm", ["pack", "--pack-destination", work, "--json"], join(packagesDir, dir));
    const { filename } = JSON.parse(out.slice(out.indexOf("{")));
    tarballs.set(manifest.name, `file:${filename}`);
  }
  const overrides =
    "# The check: Hypha's packages from this commit, not the registry.\noverrides:\n" +
    [...tarballs]
      .map(([name, spec]) => `  ${JSON.stringify(name)}: ${JSON.stringify(spec)}\n`)
      .join("");

  const { STARTERS } = await import(join(repo, "packages", "cli", "src", "create.ts"));
  for (const name of Object.keys(STARTERS)) {
    if (only && !only.includes(name)) continue;
    const dir = join(work, `my-${name}`);
    console.log(`\n== ${name}: ${STARTERS[name].path} → ${dir}`);
    run(
      "node",
      [join(repo, "packages", "cli", "src", "main.ts"), "create", name, dir, "--yes"],
      repo,
    );
    appendFileSync(join(dir, "pnpm-workspace.yaml"), overrides);
    run("pnpm", ["install", "--no-frozen-lockfile"], dir);
    const scripts = JSON.parse(readFileSync(join(dir, "package.json"), "utf8")).scripts ?? {};
    for (const script of ["typecheck", "test", "build"]) {
      if (!scripts[script]) continue;
      console.log(`-- pnpm ${script}`);
      execFileSync("pnpm", [script], { cwd: dir, stdio: "inherit" });
    }
    if (name === "storefront" && !existsSync(join(dir, "dist")))
      throw new Error("the storefront's build left no dist/");
  }
  console.log("\nok: starters");
} finally {
  rmSync(work, { recursive: true, force: true });
  for (const dir of readdirSync(packagesDir))
    for (const name of ["LICENSE-MIT", "LICENSE-APACHE"])
      rmSync(join(packagesDir, dir, name), { force: true });
}
