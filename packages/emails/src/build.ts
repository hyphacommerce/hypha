// Writes the default email templates to crates/hypha-notifications/defaults/ (other
// languages than English in a folder each) and the Rust index of them,
// crates/hypha-notifications/src/generated/defaults.rs, which the crate embeds. With
// --check, fails instead of writing when a committed file is out of date (`pnpm
// emails:check`, in CI).
//
// The templates are TSX, which Node doesn't run by itself: jiti compiles them on load.

import { mkdir, readFile, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { createJiti } from "jiti";

const jiti = createJiti(import.meta.url, { jsx: { runtime: "automatic" } });
const { renderAll } = await jiti.import<typeof import("./templates.tsx")>("./templates.tsx");

const out = new URL("../../../crates/hypha-notifications/", import.meta.url);
const check = process.argv.includes("--check");

const stale: string[] = [];
for (const [name, text] of await renderAll()) {
  const target = new URL(name, out);
  if (check) {
    const current = await readFile(target, "utf8").catch(() => "");
    if (current !== text) stale.push(fileURLToPath(target));
  } else {
    await mkdir(new URL(".", target), { recursive: true });
    await writeFile(target, text);
    console.log(`wrote ${fileURLToPath(target)}`);
  }
}
if (stale.length > 0) {
  console.error(`out of date (run \`pnpm emails:build\`):\n  ${stale.join("\n  ")}`);
  process.exit(1);
}
