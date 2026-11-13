import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";

import { createTranslator, mergeCatalogs } from "@hyphacommerce/i18n";

import { describeWith } from "./errors.ts";
import type { TFor } from "./messages";

const errors: Record<string, string> = JSON.parse(
  readFileSync(join(import.meta.dirname, "..", "messages", "en", "errors.json"), "utf8"),
);
const catalog = mergeCatalogs({ errors });

/** A translator over the `errors` namespace alone, which is all `describeWith` reads. */
function translator(language: string, messages: Record<string, string> = catalog): TFor<"errors"> {
  return createTranslator(language, messages, { fallback: catalog });
}

const en = translator("en");
const error = (code: string, text = "the api's words", details?: unknown) => ({
  code,
  error: text,
  details,
});

test("a code says its sentence from the catalog", () => {
  assert.equal(describeWith(en, error("invalid_credentials")), "Wrong email or password.");
});

test("a sentence with braces in it is shown with them", () => {
  assert.equal(
    describeWith(en, error("invalid_order_url")),
    "The order link must be an http(s) URL with {order} in it, and no placeholders but {order} and {token}.",
  );
});

test("an unknown code shows the API's words in English", () => {
  assert.equal(describeWith(en, error("brand_new_code", "something new")), "something new");
});

test("an unknown code shows a general sentence, then the API's words, in another language", () => {
  const de = translator("de", { "errors.unknown": "Etwas ist schiefgelaufen. ({error})" });
  assert.equal(
    describeWith(de, error("brand_new_code", "something new")),
    "Etwas ist schiefgelaufen. (something new)",
  );
});

test("a code another language hasn't translated reads from English", () => {
  const de = translator("de", {});
  assert.equal(describeWith(de, error("invalid_credentials")), "Wrong email or password.");
});

test("a code named after an object's property isn't a message", () => {
  assert.equal(describeWith(en, error("toString", "odd")), "odd");
});

test("codes whose detail is in the API's sentence show it as a sentence", () => {
  assert.equal(describeWith(en, error("field_too_long", "name is too long")), "Name is too long.");
  assert.equal(
    describeWith(en, error("template_too_large", "the template is over 256 KiB")),
    "The template is over 256 KiB. Shorten it.",
  );
});

test("a refused payment says the provider's reason when there is one", () => {
  assert.equal(describeWith(en, error("payment_refused")), "The provider refused.");
  assert.equal(
    describeWith(en, error("payment_refused", "x", { reason: "card declined" })),
    "The provider refused: card declined",
  );
});

test("shipping more than is left says how many are left", () => {
  assert.equal(describeWith(en, error("over_fulfilled")), "That's more than is left to ship.");
  assert.equal(
    describeWith(en, error("over_fulfilled", "x", { unfulfilled: 2 })),
    "That's more than is left to ship: 2 of that item.",
  );
});

const sentence = (details?: unknown) =>
  describeWith(en, error("custom_field_values_invalid", "x", details));

test("stored values breaking new rules are counted", () => {
  assert.equal(
    sentence(),
    "Some stored values break the new rules. Fix them first, or loosen the rules.",
  );
  assert.equal(
    sentence({ count: 1 }),
    "1 stored value breaks the new rules. Fix them first, or loosen the rules.",
  );
  assert.equal(
    sentence({ count: 1200 }),
    "1,200 stored values break the new rules. Fix them first, or loosen the rules.",
  );
});

test("a rate limit says how long to wait when the API does", () => {
  assert.equal(describeWith(en, error("rate_limited")), "Too many attempts.");
  assert.equal(
    describeWith(en, error("rate_limited", "x", { retry_after_secs: 30 })),
    "Too many attempts. Try again in 30 seconds.",
  );
});

test("a broken template names the part and the line", () => {
  assert.equal(
    describeWith(en, error("template_invalid")),
    "The template doesn't work. Check its variables and tags.",
  );
  assert.equal(
    describeWith(
      en,
      error("template_invalid", "x", { part: "html", line: 4, message: "unknown tag" }),
    ),
    "The template doesn't work. HTML, line 4: unknown tag",
  );
});

test("every sentence formats without values it wasn't given", () => {
  const needValues = new Set([
    "from_api",
    "payment_refused_reason",
    "shipping_provider_failed",
    "shipping_provider_timeout",
    "over_fulfilled_left",
    "invalid_payment_settings",
    "template_invalid_problem",
    "template_too_large",
    "email_unavailable",
    "email_refused",
    "custom_field_values_invalid_count",
    "rate_limited_seconds",
    "unknown",
  ]);
  for (const code of Object.keys(errors)) {
    if (needValues.has(code)) continue;
    assert.equal(describeWith(en, error(code)).includes("{"), code === "invalid_order_url", code);
  }
});
