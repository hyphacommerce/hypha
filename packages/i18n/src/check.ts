// The catalog check: what `hypha-i18n-check` runs over an app's `messages` directory.
// The source language (English) says which keys exist and what each takes; every other
// language may be behind it, never beside it.

import { type MessageFormatElement, TYPE } from "@formatjs/icu-messageformat-parser";
import { IntlMessageFormat } from "intl-messageformat";

export type ProblemCode =
  /** The message isn't a string, or isn't ICU MessageFormat. */
  | "invalid_message"
  /** A key the source language doesn't have. */
  | "unknown_key"
  /** Placeholders or tags other than the source's for the key. */
  | "placeholder_mismatch"
  /** A language that must be complete lacks the key. */
  | "missing_key"
  /** A language named (the source, or one that must be complete) has no catalog. */
  | "missing_language";

export interface Problem {
  code: ProblemCode;
  language: string;
  /** The key, or undefined for a problem with the language as a whole. */
  key?: string;
  message: string;
}

export interface CheckOptions {
  /** The language every other is held to. `en` by default. */
  source?: string;
  /** Languages that must have every key of the source. */
  complete?: readonly string[];
}

export interface CheckReport {
  source: string;
  /** What fails the check. */
  problems: Problem[];
  /** Per language other than the source, the source's keys it still lacks. */
  missing: Record<string, string[]>;
}

/** What a message takes: its placeholders' names and its tags', each sorted. */
export interface Placeholders {
  arguments: string[];
  tags: string[];
}

/**
 * The placeholders and tags of an ICU message, in every branch of its plurals and
 * selects. Throws what the parser throws when it isn't one.
 */
export function placeholdersOf(message: string): Placeholders {
  const names = new Set<string>();
  const tags = new Set<string>();
  const walk = (elements: MessageFormatElement[]): void => {
    for (const element of elements) {
      if (element.type === TYPE.literal || element.type === TYPE.pound) continue;
      if (element.type === TYPE.tag) {
        tags.add(element.value);
        walk(element.children);
        continue;
      }
      names.add(element.value);
      if (element.type === TYPE.plural || element.type === TYPE.select) {
        for (const option of Object.values(element.options)) walk(option.value);
      }
    }
  };
  walk(new IntlMessageFormat(message, "en").getAst());
  return { arguments: [...names].toSorted(), tags: [...tags].toSorted() };
}

/**
 * Checks the catalogs of an app, by language. Fails (a problem) on: a message that
 * isn't ICU MessageFormat, a key the source lacks, placeholders or tags that differ
 * from the source's, and a missing key in a language that must be complete. What other
 * languages still lack is reported, not a problem. An empty message isn't a translation:
 * the key counts as lacking, and an empty message in the source is a problem.
 */
export function checkCatalogs(
  catalogs: Readonly<Record<string, Readonly<Record<string, unknown>>>>,
  options: CheckOptions = {},
): CheckReport {
  const source = options.source ?? "en";
  const complete = new Set(options.complete);
  const problems: Problem[] = [];
  const missing: Record<string, string[]> = {};

  const parsed = new Map<string, Map<string, Placeholders>>();
  for (const [language, catalog] of Object.entries(catalogs)) {
    const messages = new Map<string, Placeholders>();
    for (const [key, message] of Object.entries(catalog)) {
      if (typeof message !== "string") {
        problems.push({ code: "invalid_message", language, key, message: "isn't a string" });
        continue;
      }
      if (isBlank(message)) {
        // Not a translation yet: counted with what the language lacks, below. The
        // source has nothing to fall back to.
        if (language === source) {
          problems.push({ code: "invalid_message", language, key, message: "is empty" });
        }
        continue;
      }
      try {
        messages.set(key, placeholdersOf(message));
      } catch (error) {
        const why = error instanceof Error ? error.message : String(error);
        problems.push({ code: "invalid_message", language, key, message: why });
      }
    }
    parsed.set(language, messages);
  }

  for (const language of [source, ...complete]) {
    if (!Object.hasOwn(catalogs, language)) {
      problems.push({ code: "missing_language", language, message: "has no catalog" });
    }
  }
  const sourceCatalog = Object.hasOwn(catalogs, source) ? catalogs[source] : undefined;
  if (sourceCatalog === undefined) return { source, problems, missing };
  const sourceParsed = parsed.get(source);

  for (const [language, catalog] of Object.entries(catalogs)) {
    if (language === source) continue;
    for (const key of Object.keys(catalog)) {
      if (!Object.hasOwn(sourceCatalog, key)) {
        problems.push({
          code: "unknown_key",
          language,
          key,
          message: `${source} has no such key`,
        });
        continue;
      }
      const expected = sourceParsed?.get(key);
      const found = parsed.get(language)?.get(key);
      // Either failed to parse: reported above.
      if (!expected || !found) continue;
      const difference = [
        ...differences("placeholder", expected.arguments, found.arguments),
        ...differences("tag", expected.tags, found.tags),
      ];
      if (difference.length > 0) {
        problems.push({
          code: "placeholder_mismatch",
          language,
          key,
          message: difference.join("; "),
        });
      }
    }
    const lacks = Object.keys(sourceCatalog)
      .filter((key) => !Object.hasOwn(catalog, key) || isBlank(catalog[key]))
      .toSorted();
    missing[language] = lacks;
    if (complete.has(language)) {
      for (const key of lacks) {
        problems.push({ code: "missing_key", language, key, message: "must be translated" });
      }
    }
  }
  return { source, problems, missing };
}

/** An empty message: a key someone added without its translation. */
function isBlank(message: unknown): boolean {
  return typeof message === "string" && message.trim() === "";
}

function differences(what: string, expected: string[], found: string[]): string[] {
  const out: string[] = [];
  const lacks = expected.filter((name) => !found.includes(name));
  const extra = found.filter((name) => !expected.includes(name));
  if (lacks.length > 0) out.push(`lacks ${what} ${lacks.join(", ")}`);
  if (extra.length > 0) out.push(`has ${what} ${extra.join(", ")} the source doesn't`);
  return out;
}
