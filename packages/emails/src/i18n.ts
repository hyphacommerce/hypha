// The defaults' words, from ICU MessageFormat catalogs: `messages/<language>/<namespace>.json`,
// each a flat object, read as `<namespace>.<key>` (the apps' convention). English is the
// source; every other catalog must have exactly its keys, and the build writes each
// language's defaults from its own.
//
// A message's arguments are Jinja the template fills when the API renders it, so only
// plain arguments (`{number}`) are allowed: a plural or a select would need the value,
// which only the API has. A sentence that needs a count is worded around it.

import { readdir, readFile } from "node:fs/promises";

export type Catalog = Readonly<Record<string, string>>;

/** Looks a message up and puts each argument's value in. */
export type T = (key: string, values?: Readonly<Record<string, string>>) => string;

const root = new URL("../messages/", import.meta.url);

/** A plain argument, the only ICU syntax the catalogs use besides text. */
const ARGUMENT = /\{([a-z_][a-z0-9_]*)\}/g;

/**
 * Refuses what plain substitution would read differently from ICU: any brace that isn't a
 * plain argument (a plural, a select, a format), and ICU's quoting (`'{` and `''`).
 */
export function checkMessage(where: string, message: string): void {
  const rest = message.replace(ARGUMENT, "");
  if (/[{}]/.test(rest)) {
    throw new Error(`${where}: only plain {arguments} are allowed: ${JSON.stringify(message)}`);
  }
  if (/'[{}']/.test(message)) {
    throw new Error(`${where}: ICU quoting isn't allowed: ${JSON.stringify(message)}`);
  }
}

function argumentsOf(message: string): string[] {
  return [...message.matchAll(ARGUMENT)].map((m) => m[1] ?? "").toSorted();
}

/** Every catalog by language: `messages/<language>/*.json` flattened to `namespace.key`. */
export async function loadCatalogs(): Promise<Map<string, Catalog>> {
  const catalogs = new Map<string, Catalog>();
  const languages = (await readdir(root, { withFileTypes: true }))
    .filter((entry) => entry.isDirectory())
    .map((entry) => entry.name)
    .toSorted();
  for (const language of languages) {
    const catalog: Record<string, string> = {};
    const files = (await readdir(new URL(`${language}/`, root))).filter((f) => f.endsWith(".json"));
    for (const file of files.toSorted()) {
      const namespace = file.replace(/\.json$/, "");
      const messages: unknown = JSON.parse(
        await readFile(new URL(`${language}/${file}`, root), "utf8"),
      );
      if (typeof messages !== "object" || messages === null || Array.isArray(messages)) {
        throw new Error(`${language}/${file}: not an object of messages`);
      }
      for (const [key, message] of Object.entries(messages)) {
        if (typeof message !== "string") throw new Error(`${language}/${file}: ${key} isn't text`);
        checkMessage(`${language}/${file}: ${key}`, message);
        catalog[`${namespace}.${key}`] = message;
      }
    }
    catalogs.set(language, catalog);
  }
  if (!catalogs.has("en")) throw new Error("messages/en is the source catalog and must exist");
  return catalogs;
}

/**
 * Holds every catalog to English: the same keys, and each message with the same
 * arguments. Returns what's wrong, empty when nothing is.
 */
export function compareCatalogs(catalogs: Map<string, Catalog>): string[] {
  const source = catalogs.get("en") ?? {};
  const problems: string[] = [];
  for (const [language, catalog] of catalogs) {
    if (language === "en") continue;
    for (const [key, message] of Object.entries(source)) {
      const translated = catalog[key];
      if (translated === undefined) {
        problems.push(`${language}: ${key} is missing`);
        continue;
      }
      const want = argumentsOf(message).join(", ");
      const got = argumentsOf(translated).join(", ");
      if (want !== got) problems.push(`${language}: ${key} has {${got}}, English has {${want}}`);
    }
    for (const key of Object.keys(catalog)) {
      if (!(key in source)) problems.push(`${language}: ${key} isn't in English`);
    }
  }
  return problems;
}

/** `t` for one catalog: an unknown key, or an argument missing or left over, throws. */
export function translator(language: string, catalog: Catalog): T {
  return (key, values = {}) => {
    const message = catalog[key];
    if (message === undefined) throw new Error(`${language}: no message ${key}`);
    const used = new Set<string>();
    const text = message.replace(ARGUMENT, (_, name: string) => {
      const value = values[name];
      if (value === undefined) throw new Error(`${language}: ${key} needs {${name}}`);
      used.add(name);
      return value;
    });
    for (const name of Object.keys(values)) {
      if (!used.has(name)) throw new Error(`${language}: ${key} doesn't take {${name}}`);
    }
    return text;
  };
}
