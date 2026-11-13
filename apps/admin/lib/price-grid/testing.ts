// For the price lists' tests under `node --test`: the English translator over the
// `price_lists` catalog, and the formats the admin writes English with.

import { readFileSync } from "node:fs";
import { join } from "node:path";

import { createTranslator, mergeCatalogs } from "@hyphacommerce/i18n";

import { createFormat } from "../format.ts";
import { formatLocale } from "../pick-language.ts";
import type { PriceT } from "./say.ts";

const price_lists: Record<string, string> = JSON.parse(
  readFileSync(join(import.meta.dirname, "..", "..", "messages", "en", "price_lists.json"), "utf8"),
);

/** The messages of `price_lists`, by full key. */
export const catalog: Readonly<Record<string, string>> = mergeCatalogs({ price_lists });

export const t: PriceT = createTranslator("en", catalog, { locale: formatLocale("en") });

export const format = createFormat(formatLocale("en"), { currencies: { EUR: 2, JPY: 0, KWD: 3 } });
