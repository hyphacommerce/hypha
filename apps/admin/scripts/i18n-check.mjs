// The admin's catalog check: `hypha-i18n-check messages` (every message is ICU
// MessageFormat, no language has a key English lacks, placeholders and tags match
// English's), with every language the admin ships held complete, since a browser gets
// its language's catalog alone, without English beside it. And the keys' own rule:
// snake_case within a namespace. Then Astryx's strings for each shipped language, with
// the admin's supplement (`astryx-messages/`), held complete against Astryx's English.

import { spawnSync } from "node:child_process";
import { readFile, readdir } from "node:fs/promises";
import { basename, join } from "node:path";
import process from "node:process";

import { checkCatalogs } from "@hyphacommerce/i18n/check";

import { SUPPLEMENTS, astryxCatalog, astryxOf, supplementOf } from "./astryx-catalogs.mjs";

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

// Astryx's strings, held the same way: for each language the admin ships, Astryx's own
// catalog with the admin's supplement over it must have every key of Astryx's English, so
// an Astryx upgrade that adds strings lists them here until they're translated.
const astryxEnglish = await astryxCatalog("en");
let astryxProblems = 0;
for (const language of languages.filter((l) => l !== SOURCE).toSorted()) {
  const name = astryxOf(language);
  const shipped = name ? await astryxCatalog(name) : {};
  const supplement = await supplementOf(language);
  const report = checkCatalogs(
    { [SOURCE]: astryxEnglish, [language]: { ...shipped, ...supplement } },
    { source: SOURCE, complete: [language] },
  );
  // What's wrong in Astryx's own catalog is Astryx's to fix; only the missing keys and the
  // supplement's own lines are the admin's.
  const problems = report.problems.filter(
    (p) => p.code === "missing_key" || (p.key !== undefined && Object.hasOwn(supplement, p.key)),
  );
  const summary = `astryx ${language}: ${Object.keys(shipped).length} from Astryx (${name ?? "none"}), ${Object.keys(supplement).length} from ${basename(SUPPLEMENTS)}/${language}.json`;
  console.log(
    problems.length === 0 ? `${summary}, complete` : `${summary}, ${problems.length} problems`,
  );
  const shadowed = Object.keys(supplement).filter((key) => Object.hasOwn(shipped, key));
  if (shadowed.length > 0) {
    console.log(`  Astryx ships these now (the supplement's lines win): ${shadowed.join(", ")}`);
  }
  for (const problem of problems) {
    const why =
      problem.code === "missing_key"
        ? `add it to ${basename(SUPPLEMENTS)}/${language}.json`
        : problem.message;
    console.error(`error: astryx ${language}: ${problem.key}: ${why} (${problem.code})`);
  }
  astryxProblems += problems.length;
}

process.exitCode =
  check.status !== 0 ? (check.status ?? 1) : badKeys > 0 || astryxProblems > 0 ? 1 : 0;
