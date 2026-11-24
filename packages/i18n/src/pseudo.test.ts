import assert from "node:assert/strict";
import { test } from "node:test";

import { placeholdersOf } from "./check.ts";
import { pseudo, pseudoMessage } from "./pseudo.ts";
import { createTranslator } from "./translator.ts";

test("accents letters and doubles vowels", () => {
  assert.equal(pseudoMessage("Your cart"), "Ýööüüŕ çááŕţ");
  assert.equal(pseudoMessage("OK 42!"), "ÖöĶ 42!");
});

test("leaves placeholders alone", () => {
  assert.equal(pseudoMessage("Hello, {name}!"), "Ĥééļļöö, {name}!");
  assert.equal(
    pseudoMessage("Total: {amount, number, ::currency/EUR}"),
    "Ţööţááļ: {amount, number, ::currency/EUR}",
  );
});

test("translates a plural's branches, not its syntax", () => {
  assert.equal(
    pseudoMessage("{count, plural, =0 {No items} one {# item} other {# items}}"),
    "{count, plural, =0 {Ñöö ííţééɱš} one {# ííţééɱ} other {# ííţééɱš}}",
  );
});

test("nested arguments inside a branch stay", () => {
  assert.equal(
    pseudoMessage(
      "{gender, select, female {{name} and her {count, plural, one {cat} other {cats}}} other {x}}",
    ),
    "{gender, select, female {{name} ááñð ĥééŕ {count, plural, one {çááţ} other {çááţš}}} other {ẋ}}",
  );
});

test("leaves tags alone, translating what's inside", () => {
  assert.equal(
    pseudoMessage("Accept the <link>terms</link>.<br/>"),
    "Ááççééþţ ţĥéé <link>ţééŕɱš</link>.<br/>",
  );
});

test("a lone < is text", () => {
  assert.equal(pseudoMessage("a < b"), "áá < ƀ");
});

test("quoted syntax stays quoted, apostrophes stay apostrophes", () => {
  assert.equal(pseudoMessage("Use '{name}' here"), "Üüšéé '{name}' ĥééŕéé");
  assert.equal(pseudoMessage("It's ours, isn''t it"), "Ííţ'š ööüüŕš, ííšñ''ţ ííţ");
});

test("a pseudo catalog has the same keys, placeholders and tags", () => {
  const en = {
    title: "Your cart",
    count: "{count, plural, one {# item} other {# items}}",
    terms: "Accept the <link>terms</link>, {name}.",
    quoted: "Braces look like '{this}', {name}",
    when: "On {at, date, long} at {at, time, short}",
  };
  const made = pseudo(en);
  assert.deepEqual(Object.keys(made), Object.keys(en));
  for (const key of Object.keys(en) as (keyof typeof en)[]) {
    assert.notEqual(made[key], en[key], key);
    assert.deepEqual(placeholdersOf(made[key]), placeholdersOf(en[key]), key);
  }
});

test("a pseudo catalog formats with the same values", () => {
  const t = createTranslator(
    "en",
    pseudo({
      count: "{count, plural, one {# item} other {# items}}",
      terms: "Accept the <link>terms</link>, {name}.",
    }),
  );
  assert.equal(t("count", { count: 1 }), "1 ííţééɱ");
  assert.equal(t("count", { count: 3 }), "3 ííţééɱš");
  assert.deepEqual(t.rich("terms", { name: "Ada", link: (chunks) => `[${chunks.join("")}]` }), [
    "Ááççééþţ ţĥéé [ţééŕɱš], Ada.",
  ]);
});

test("text gets about a third longer", () => {
  const text = "Add the selected products to your cart and continue to checkout";
  const ratio = pseudoMessage(text).length / text.length;
  assert.ok(ratio > 1.2 && ratio < 1.6, String(ratio));
});
