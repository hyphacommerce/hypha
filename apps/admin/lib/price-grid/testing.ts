// For the price lists' tests under `node --test`: the English translator over the
// `price_lists` and `components` catalogs, and the formats the admin writes English with.

import { readFileSync } from "node:fs";
import { join } from "node:path";

import { createTranslator, mergeCatalogs } from "@hyphacommerce/i18n";

import { createFormat } from "../format.ts";
import { formatLocale } from "../pick-language.ts";
import type { PriceT } from "./types.ts";

function read(namespace: string): Record<string, string> {
  const file = join(import.meta.dirname, "..", "..", "messages", "en", `${namespace}.json`);
  const parsed: Record<string, string> = JSON.parse(readFileSync(file, "utf8"));
  return parsed;
}

/** The messages of `price_lists` and the amount parser's (`components`), by full key. */
export const catalog: Readonly<Record<string, string>> = mergeCatalogs({
  components: read("components"),
  price_lists: read("price_lists"),
});

export const t: PriceT = createTranslator("en", catalog, { locale: formatLocale("en") });

export const format = createFormat(formatLocale("en"), { currencies: { EUR: 2, JPY: 0, KWD: 3 } });
