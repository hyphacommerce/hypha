// The admin's catalog check: `hypha-i18n-check messages` (every message is ICU
// MessageFormat, no language has a key English lacks, placeholders and tags match
// English's), with every language the admin ships held complete, since a browser gets
// its language's catalog alone, without English beside it. And the keys' own rule:
// snake_case within a namespace.

import { spawnSync } from "node:child_process";
import { readFile, readdir } from "node:fs/promises";
import { join } from "node:path";
import process from "node:process";

const SOURCE = "en";
const KEY = /^[a-z][a-z0-9]*(_[a-z0-9]+)*$/;
const app = join(import.meta.dirname, "..");
const root = join(app, "messages");

const entries = await readdir(root, { withFileTypes: true });
const languages = entries.filter((entry) => entry.isDirectory()).map((entry) => entry.name);

let badKeys = 0;
for (const file of await readdir(join(root, SOURCE))) {
  if (!file.endsWith(".json")) continue;
  const messages = JSON.parse(await readFile(join(root, SOURCE, file), "utf8"));
  for (const key of Object.keys(messages)) {
    if (KEY.test(key)) continue;
    badKeys += 1;
    console.error(`error: ${SOURCE}/${file}: ${JSON.stringify(key)} isn't snake_case`);
  }
}

const complete = languages.flatMap((language) =>
  language === SOURCE ? [] : ["--complete", language],
);
const check = spawnSync(
  join(app, "node_modules", ".bin", "hypha-i18n-check"),
  ["messages", "--source", SOURCE, ...complete],
  { cwd: app, stdio: "inherit", shell: process.platform === "win32" },
);
if (check.error) throw check.error;
process.exitCode = check.status !== 0 ? (check.status ?? 1) : badKeys > 0 ? 1 : 0;
