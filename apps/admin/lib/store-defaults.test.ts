import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";

import { createTranslator, mergeCatalogs } from "@hyphacommerce/i18n";

import {
  PAYMENT_NAME_DEFAULTS,
  type StoreT,
  pickDefaults,
  storeLanguage,
  storeTranslator,
} from "./store-defaults.ts";

const defaults: Record<string, string> = JSON.parse(
  readFileSync(join(import.meta.dirname, "..", "messages", "en", "defaults.json"), "utf8"),
);
const english = mergeCatalogs({ defaults });

// A made-up second language, with a catalog for two of the defaults.
const FIXTURE = "sr-Latn";
const catalogs: Record<string, Record<string, string>> = {
  en: english,
  [FIXTURE]: {
    "defaults.payment_cash_on_delivery_name": "Plaćanje pouzećem",
    "defaults.discount_free_shipping_title": "Besplatna dostava",
  },
};
const available = Object.keys(catalogs);

/** As the server's `getT`: the language's catalog over English. */
function translatorFor(language: string): Promise<StoreT> {
  const translator: StoreT = createTranslator(language, catalogs[language] ?? english, {
    fallback: english,
  });
  return Promise.resolve(translator);
}

test("a store in a language with a catalog gets that language's defaults", async () => {
  const t = await storeTranslator(FIXTURE, available, translatorFor);
  assert.equal(t.language, FIXTURE);
  assert.equal(t("defaults.payment_cash_on_delivery_name"), "Plaćanje pouzećem");
  assert.equal(t("defaults.discount_free_shipping_title"), "Besplatna dostava");
  // What the language's catalog lacks reads from English.
  assert.equal(t("defaults.payment_bank_transfer_name"), "Bank transfer");
  assert.deepEqual(pickDefaults(t, PAYMENT_NAME_DEFAULTS), {
    payment_cash_on_delivery_name: "Plaćanje pouzećem",
    payment_bank_transfer_name: "Bank transfer",
  });
});

test("a nearer form of the store's language is used", () => {
  assert.equal(storeLanguage("sr", available), FIXTURE);
  assert.equal(storeLanguage("sr-latn", available), FIXTURE);
  assert.equal(storeLanguage("en-GB", available), "en");
});

test("a language the admin has no catalog for falls back to English", async () => {
  assert.equal(storeLanguage("ja", available), "en");
  const t = await storeTranslator("ja", available, translatorFor);
  assert.equal(t.language, "en");
  assert.equal(t("defaults.payment_cash_on_delivery_name"), "Cash on delivery");
  assert.equal(t("defaults.channel_key_name", { channel: "Online" }), "Online storefront");
});

test("a store that couldn't be read gets English", () => {
  assert.equal(storeLanguage(undefined, available), "en");
  assert.equal(storeLanguage(null, available), "en");
});
