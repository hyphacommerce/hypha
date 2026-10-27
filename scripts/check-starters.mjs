// Checks the starters work outside the workspace, as `hypha create` hands them out: each
// is created by the CLI from this checkout into a temporary folder, its `@hyphacommerce/*`
// dependencies pointed at the packages packed from this commit (`pnpm packages:build`
// first), installed, type-checked, tested and built. Run by CI after `packages:check`
// (the same built packages); `pnpm starters:check` locally. `STARTERS=app` checks only
// those named.

import { execFileSync } from "node:child_process";
import { appendFileSync, existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

import { cleanLicenses, overridesYaml, packPackages, repo } from "./lib/pack-packages.mjs";

const work = mkdtempSync(join(tmpdir(), "hypha-starters-"));
const run = (cmd, args, cwd = work) =>
  execFileSync(cmd, args, { cwd, stdio: ["ignore", "pipe", "inherit"], encoding: "utf8" });
const only = process.env.STARTERS?.split(",")
  .map((s) => s.trim())
  .filter(Boolean);

try {
  const tarballs = packPackages(work);
  const overrides =
    "# The check: Hypha's packages from this commit, not the registry.\n" + overridesYaml(tarballs);

  const { STARTERS } = await import(
    pathToFileURL(join(repo, "packages", "cli", "src", "create.ts")).href
  );
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
  cleanLicenses();
}
