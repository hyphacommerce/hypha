// Bundles each spore's storefront elements (`spores/*/element/*.ts`) into the `.js`
// beside it, which the spore serves (`include_str!`, design note 0030, §7). With
// --check, fails instead of writing when a committed bundle is out of date (CI runs it).

import { readFile, readdir, writeFile } from "node:fs/promises";
import { join } from "node:path";

import { build } from "vite-plus";

const repo = join(import.meta.dirname, "..");
const root = join(repo, "spores");
const check = process.argv.includes("--check");

async function bundle(entry) {
  const result = await build({
    // The checkout's root, whatever the working directory: the output names paths
    // relative to it, so a build from anywhere gives the same bytes.
    root: repo,
    configFile: false,
    logLevel: "silent",
    build: {
      write: false,
      minify: true,
      target: "es2022",
      lib: { entry, formats: ["es"], fileName: "element" },
      // A library's ES output keeps its whitespace unless told: this one is served.
      rolldownOptions: { output: { minify: true, comments: false } },
    },
  });
  const outputs = (Array.isArray(result) ? result : [result]).flatMap((r) => r.output);
  const chunk = outputs.find((o) => o.type === "chunk" && o.isEntry);
  if (!chunk) throw new Error(`nothing was bundled for ${entry}`);
  return chunk.code;
}

let stale = [];
for (const spore of await readdir(root)) {
  const dir = join(root, spore, "element");
  const files = await readdir(dir).catch(() => []);
  for (const file of files.filter((f) => f.endsWith(".ts"))) {
    const target = join(dir, file.replace(/\.ts$/, ".js"));
    const code = await bundle(join(dir, file));
    const current = await readFile(target, "utf8").catch(() => undefined);
    if (current === code) continue;
    if (check) stale.push(target);
    else {
      await writeFile(target, code);
      console.log(`wrote ${target}`);
    }
  }
}
if (stale.length) {
  console.error(`out of date (run \`pnpm spores:build\`):\n${stale.join("\n")}`);
  process.exit(1);
}
