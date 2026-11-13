// Astryx's own strings for the admin's languages, shared by the index (which writes the
// loaders) and the check (which holds each shipped language to Astryx's English): the
// catalog Astryx ships for a language, and the admin's supplement for the keys it lacks
// (`astryx-messages/<language>.json`, outside `messages/`, whose folders are languages).

import { readFile, readdir } from "node:fs/promises";
import { createRequire } from "node:module";
import { join } from "node:path";

/** The pseudo-language (`lib/pseudo.ts`): no folder, English accented in development. */
export const PSEUDO = "en-XA";

/** The admin's own Astryx strings, a flat `{key: message}` file per language. */
export const SUPPLEMENTS = join(import.meta.dirname, "..", "astryx-messages");

/**
 * Astryx's shipped catalogs (`@astryxdesign/core/locales/<name>.generated.js`, named by
 * Crowdin's codes) by the tags the admin's languages are matched with: the exact tag
 * first, then the tag without its last parts.
 */
const ASTRYX = {
  af: "af-ZA",
  ar: "ar-SA",
  ca: "ca-ES",
  cs: "cs-CZ",
  da: "da-DK",
  de: "de-DE",
  el: "el-GR",
  es: "es-ES",
  fi: "fi-FI",
  fr: "fr-FR",
  he: "he-IL",
  hu: "hu-HU",
  it: "it-IT",
  ja: "ja-JP",
  ko: "ko-KR",
  nb: "no-NO",
  nl: "nl-NL",
  no: "no-NO",
  pl: "pl-PL",
  pt: "pt-PT",
  "pt-BR": "pt-BR",
  ro: "ro-RO",
  ru: "ru-RU",
  // Cyrillic only; the admin reads it in Latin for `sr-Latn` (`lib/astryx-locales.ts`).
  sr: "sr-SP",
  sv: "sv-SE",
  tr: "tr-TR",
  uk: "uk-UA",
  vi: "vi-VN",
  zh: "zh-CN",
  "zh-Hant": "zh-TW",
  "zh-HK": "zh-TW",
  "zh-TW": "zh-TW",
  [PSEUDO]: "pseudo",
};

/** Astryx's catalog name for `language`, if it ships one. */
export function astryxOf(language) {
  const parts = language.split("-");
  for (let length = parts.length; length > 0; length -= 1) {
    const name = ASTRYX[parts.slice(0, length).join("-")];
    if (name) return name;
  }
  return undefined;
}

/** The languages with a supplement, from the file names. */
export async function supplementedLanguages() {
  const files = await readdir(SUPPLEMENTS).catch(() => []);
  return files
    .filter((file) => file.endsWith(".json"))
    .map((file) => file.slice(0, -".json".length))
    .toSorted();
}

/** A language's supplement, `{}` when it has none. */
export async function supplementOf(language) {
  const text = await readFile(join(SUPPLEMENTS, `${language}.json`), "utf8").catch(() => null);
  return text === null ? {} : JSON.parse(text);
}

const require = createRequire(import.meta.url);

/**
 * An authored Astryx catalog (`en` or a Crowdin name) as `{key: message}`. The authored
 * JSON has each message with its translator's description; the runtime chunks don't.
 */
export async function astryxCatalog(name) {
  const path = require.resolve(`@astryxdesign/core/locales/${name}.json`);
  const authored = JSON.parse(await readFile(path, "utf8"));
  return Object.fromEntries(
    Object.entries(authored).map(([key, entry]) => [key, entry.defaultMessage]),
  );
}
