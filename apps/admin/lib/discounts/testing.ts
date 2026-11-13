// For this folder's tests: the English translator over the catalogs these modules read,
// and the formats an English reader gets. Never imported by the app.

import { readFileSync } from "node:fs";
import { join } from "node:path";

import { createTranslator, mergeCatalogs } from "@hyphacommerce/i18n";

import { createFormat } from "../format.ts";
import { formatLocale } from "../pick-language.ts";
import type { DiscountsT } from "./validate.ts";

function catalog(namespace: string): Record<string, string> {
  const file = join(import.meta.dirname, "..", "..", "messages", "en", `${namespace}.json`);
  const parsed: Record<string, string> = JSON.parse(readFileSync(file, "utf8"));
  return parsed;
}

const messages = mergeCatalogs({ common: catalog("common"), discounts: catalog("discounts") });

/** English; a message that can't be formatted throws. */
export const t: DiscountsT = createTranslator("en", messages, {
  fallback: messages,
  locale: formatLocale("en"),
});

export const format = createFormat(t.locale);
