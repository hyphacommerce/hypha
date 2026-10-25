// Checks the npm packages work from outside the workspace, as a user installs them: each
// publishable package is built (`pnpm packages:build` first) and packed, the tarballs are
// installed into a temporary project (the packages' dependencies on each other pointed at
// the tarballs), every export is imported by Node, and a TypeScript file importing them
// type-checks against the shipped declarations. Run by CI; `pnpm packages:check` locally.

import { execFileSync } from "node:child_process";
import { cpSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const repo = fileURLToPath(new URL("..", import.meta.url));
const packagesDir = join(repo, "packages");
const work = mkdtempSync(join(tmpdir(), "hypha-packages-"));
const run = (cmd, args, cwd = work) =>
  execFileSync(cmd, args, { cwd, stdio: ["ignore", "pipe", "inherit"], encoding: "utf8" });

try {
  // Publishable packages: not `private`.
  const packages = readdirSync(packagesDir)
    .map((dir) => ({ dir: join(packagesDir, dir), manifest: null }))
    .map((p) => ({ ...p, manifest: JSON.parse(readFileSync(join(p.dir, "package.json"), "utf8")) }))
    .filter((p) => !p.manifest.private);

  const tarballs = new Map();
  for (const p of packages) {
    // The licence files travel with each package (listed in its `files`).
    for (const name of ["LICENSE-MIT", "LICENSE-APACHE"])
      cpSync(join(repo, name), join(p.dir, name));
    const out = run("pnpm", ["pack", "--pack-destination", work, "--json"], p.dir);
    const { filename } = JSON.parse(out.slice(out.indexOf("{")));
    tarballs.set(p.manifest.name, `file:${filename}`);
  }

  // Peer dependencies the exports need at import time.
  const peers = {
    react: "19.3.1",
    "react-dom": "19.3.1",
    "@astryxdesign/core": "0.7.1",
    typescript: "5.9.3",
    "@types/react": "19.3.0",
  };
  writeFileSync(
    join(work, "package.json"),
    JSON.stringify(
      {
        name: "hypha-packages-check",
        private: true,
        type: "module",
        dependencies: { ...Object.fromEntries(tarballs), ...peers },
      },
      null,
      2,
    ),
  );
  // The packages' dependencies on each other resolve to the tarballs, not the registry
  // (pnpm reads overrides from pnpm-workspace.yaml).
  writeFileSync(
    join(work, "pnpm-workspace.yaml"),
    'minimumReleaseAge: 0\n# Astryx\'s install script only prints a hint.\nallowBuilds:\n  "@astryxdesign/core": false\noverrides:\n' +
      [...tarballs]
        .map(([name, spec]) => `  ${JSON.stringify(name)}: ${JSON.stringify(spec)}\n`)
        .join(""),
  );
  run("pnpm", ["install", "--no-frozen-lockfile"]);

  // Every export of every package is importable by Node and typed.
  const imports = [];
  for (const p of packages) {
    const exports = p.manifest.publishConfig?.exports ?? p.manifest.exports;
    for (const sub of Object.keys(exports)) {
      if (sub.endsWith(".json")) continue;
      imports.push(`${p.manifest.name}${sub.slice(1)}`);
    }
  }
  for (const spec of imports) {
    run("node", ["--input-type=module", "-e", `await import(${JSON.stringify(spec)})`]);
    console.log("imported", spec);
  }
  writeFileSync(
    join(work, "check.ts"),
    imports.map((s) => `import ${JSON.stringify(s)};`).join("\n") + "\n",
  );
  writeFileSync(
    join(work, "tsconfig.json"),
    JSON.stringify({
      compilerOptions: {
        module: "ESNext",
        moduleResolution: "Bundler",
        target: "ES2022",
        strict: true,
        noEmit: true,
        skipLibCheck: true,
        jsx: "react-jsx",
        types: [],
      },
      files: ["check.ts"],
    }),
  );
  run("pnpm", ["exec", "tsc", "-p", "tsconfig.json"]);
  console.log(`ok: ${packages.length} packages, ${imports.length} exports`);
} finally {
  rmSync(work, { recursive: true, force: true });
  for (const dir of readdirSync(packagesDir)) {
    for (const name of ["LICENSE-MIT", "LICENSE-APACHE"])
      rmSync(join(packagesDir, dir, name), { force: true });
  }
}
