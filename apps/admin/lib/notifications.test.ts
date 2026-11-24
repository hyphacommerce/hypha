import assert from "node:assert/strict";
import { test } from "node:test";

import { isTranslated, templateHref, templateLanguage, templateQuery } from "./notifications.ts";

test("buyer templates and the layout are written per language", () => {
  assert.equal(isTranslated("buyer"), true);
  assert.equal(isTranslated(null), true);
  assert.equal(isTranslated(undefined), true);
});

test("staff, supplier and unknown audiences are in the content language only", () => {
  assert.equal(isTranslated("staff"), false);
  assert.equal(isTranslated("supplier"), false);
  assert.equal(isTranslated("partner"), false);
});

test("a template page's language is one of the store's others, in its own case", () => {
  assert.equal(templateLanguage("SR-latn", ["de", "sr-Latn"]), "sr-Latn");
  assert.equal(templateLanguage("de", ["de", "sr-Latn"]), "de");
});

test("no language, the content language or one the store lacks is the content language", () => {
  assert.equal(templateLanguage(undefined, ["sr-Latn"]), null);
  assert.equal(templateLanguage("en", ["sr-Latn"]), null);
  assert.equal(templateLanguage("fr", ["sr-Latn"]), null);
  assert.equal(templateLanguage("sr", ["sr-Latn"]), null);
});

test("a template's routes name the language only when it isn't the content language", () => {
  assert.deepEqual(templateQuery(null), {});
  assert.deepEqual(templateQuery("sr-Latn"), { language: "sr-Latn" });
});

test("a template's editor names its language only when it isn't the content language", () => {
  assert.equal(templateHref("order_shipped", null), "/settings/notifications/order_shipped");
  assert.equal(
    templateHref("order_shipped", "sr-Latn"),
    "/settings/notifications/order_shipped?language=sr-Latn",
  );
});
