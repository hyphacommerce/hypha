#!/usr/bin/env node
// hypha-i18n-check <messages> [--source en] [--complete <language>]…
//
// Checks an app's catalogs: `<messages>/<language>/<namespace>.json` (keys become
// `<namespace>.<key>`), or `<messages>/<language>.json` with full keys: one layout or
// the other, never both in one directory. Every message
// must be ICU MessageFormat; a language other than the source may only have the source's
// keys, with the source's placeholders and tags. What each language still lacks is
// printed, and fails only for a language named with --complete. Exits 1 on a failure,
// 2 when it couldn't run.

import { readFile, readdir } from "node:fs/promises";
import { join } from "node:path";
import { parseArgs } from "node:util";

import pkg from "../package.json" with { type: "json" };
import { checkCatalogs } from "../src/check.ts";

const USAGE = "usage: hypha-i18n-check <messages> [--source en] [--complete <language>]…";

/** Why the directory couldn't be read as catalogs. */
class UsageError extends Error {}

/** The catalogs under `dir`, by language. */
async function readCatalogs(dir: string): Promise<Record<string, Record<string, unknown>>> {
  const catalogs: Record<string, Record<string, unknown>> = {};
  const all = await readdir(dir, { withFileTypes: true }).catch(() => {
    throw new UsageError(`${dir} isn't a directory`);
  });
  const entries = all
    .filter((entry) => !entry.name.startsWith("."))
    .filter((entry) => entry.isDirectory() || entry.name.endsWith(".json"))
    .toSorted((a, b) => a.name.localeCompare(b.name));
  const folder = entries.find((entry) => entry.isDirectory());
  const loose = entries.find((entry) => !entry.isDirectory());
  if (folder && loose) {
    throw new UsageError(
      `${dir} mixes two layouts: ${folder.name}/ (a folder per language) and ${loose.name} (a file per language). Keep one.`,
    );
  }
  for (const entry of entries) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      const catalog: Record<string, unknown> = {};
      catalogs[entry.name] = catalog;
      const files = (await readdir(path)).filter((name) => name.endsWith(".json")).toSorted();
      for (const file of files) {
        const namespace = file.slice(0, -".json".length);
        for (const [key, message] of Object.entries(await readJson(join(path, file)))) {
          catalog[`${namespace}.${key}`] = message;
        }
      }
    } else {
      catalogs[entry.name.slice(0, -".json".length)] = await readJson(path);
    }
  }
  return catalogs;
}

async function readJson(path: string): Promise<Record<string, unknown>> {
  let value: unknown;
  try {
    value = JSON.parse(await readFile(path, "utf8"));
  } catch (error) {
    throw new UsageError(`${path}: ${error instanceof Error ? error.message : String(error)}`);
  }
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new UsageError(`${path}: a catalog is an object of key: message`);
  }
  return { ...value };
}

async function main(argv: string[]): Promise<number> {
  const { values, positionals } = parseArgs({
    args: argv,
    allowPositionals: true,
    options: {
      source: { type: "string", default: "en" },
      complete: { type: "string", multiple: true, default: [] },
      version: { type: "boolean", default: false },
      help: { type: "boolean", short: "h", default: false },
    },
  });
  if (values.version) {
    console.log(pkg.version);
    return 0;
  }
  if (values.help) {
    console.log(USAGE);
    return 0;
  }
  const [dir, ...rest] = positionals;
  if (dir === undefined || rest.length > 0) throw new UsageError(USAGE);

  const catalogs = await readCatalogs(dir);
  const report = checkCatalogs(catalogs, { source: values.source, complete: values.complete });

  for (const language of Object.keys(catalogs)) {
    const count = Object.keys(catalogs[language] ?? {}).length;
    const messages = `${language}: ${count} message${count === 1 ? "" : "s"}`;
    if (language === report.source) {
      console.log(`${messages} (the source)`);
      continue;
    }
    const missing = report.missing[language] ?? [];
    console.log(
      missing.length === 0 ? `${messages}, complete` : `${messages}, ${missing.length} missing`,
    );
    for (const key of missing) console.log(`  ${key}`);
  }
  for (const problem of report.problems) {
    const where =
      problem.key === undefined ? problem.language : `${problem.language}: ${problem.key}`;
    console.error(`error: ${where}: ${problem.message} (${problem.code})`);
  }
  if (report.problems.length > 0) {
    console.error(`${report.problems.length} problem${report.problems.length === 1 ? "" : "s"}`);
    return 1;
  }
  return 0;
}

try {
  process.exitCode = await main(process.argv.slice(2));
} catch (error) {
  // Bad arguments (parseArgs throws a TypeError) or a directory that isn't catalogs.
  if (!(error instanceof UsageError || error instanceof TypeError)) throw error;
  console.error(error.message);
  process.exitCode = 2;
}
