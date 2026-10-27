// Checks the npm packages work from outside the workspace, as a user installs them: each
// publishable package is built (`pnpm packages:build` first) and packed, the tarballs are
// installed into a temporary project (the packages' dependencies on each other pointed at
// the tarballs), every export is imported by Node, and a TypeScript file importing them
// type-checks against the shipped declarations. Run by CI; `pnpm packages:check` locally.

import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import {
  cleanLicenses,
  overridesYaml,
  packPackages,
  publishablePackages,
} from "./lib/pack-packages.mjs";

const work = mkdtempSync(join(tmpdir(), "hypha-packages-"));
const run = (cmd, args, cwd = work) =>
  execFileSync(cmd, args, { cwd, stdio: ["ignore", "pipe", "inherit"], encoding: "utf8" });

try {
  const packages = publishablePackages();
  const tarballs = packPackages(work);

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
    'minimumReleaseAge: 0\n# Astryx\'s install script only prints a hint.\nallowBuilds:\n  "@astryxdesign/core": false\n' +
      overridesYaml(tarballs),
  );
  run("pnpm", ["install", "--no-frozen-lockfile"]);

  // Every export of every package is importable by Node and typed; a package with a
  // binary (the CLI) runs it, outside any checkout.
  const imports = [];
  for (const p of packages) {
    const bin = p.manifest.publishConfig?.bin ?? p.manifest.bin;
    for (const name of Object.keys(bin ?? {})) {
      const out = run("pnpm", ["exec", name, "--version"]);
      if (!out.includes(p.manifest.version))
        throw new Error(`${name} --version printed ${out.trim()}`);
      console.log("ran", name, out.trim());
    }
    const exports = p.manifest.publishConfig?.exports ?? p.manifest.exports ?? {};
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
  cleanLicenses();
}
