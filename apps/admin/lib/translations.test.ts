import assert from "node:assert/strict";
import { test } from "node:test";

import {
  type TranslatableField,
  canReadTranslations,
  canWriteTranslations,
  describeField,
  draftOf,
  grouped,
  isDirty,
  isKind,
  isTranslatable,
  lengthOf,
  problemsOf,
  putBody,
  resourceHref,
  translateHref,
} from "./translations.ts";

function field(
  key: string,
  source: string,
  translations: TranslatableField["translations"] = {},
  format = "text",
): TranslatableField {
  return { key, format, max_length: 255, source, digest: `digest-of-${key}`, translations };
}

const optionOf = (id: string) => (id === "optval_1" ? "Size" : undefined);

const upToDate = (value: string, key: string) => ({
  value,
  source_digest: `digest-of-${key}`,
  outdated: false,
});
const outdated = (value: string) => ({ value, source_digest: "an-older-digest", outdated: true });

test("a kind is one of the API's ten", () => {
  assert.equal(isKind("product"), true);
  assert.equal(isKind("search_facet"), true);
  assert.equal(isKind("custom_field_value"), false);
  assert.equal(isKind(""), false);
});

test("reading translations takes translations:read and the kind's own read scope", () => {
  assert.equal(canReadTranslations(["translations:read", "products:read"], "product"), true);
  assert.equal(canReadTranslations(["translations:read"], "product"), false);
  assert.equal(canReadTranslations(["products:read"], "product"), false);
  // A write scope implies its read scope.
  assert.equal(canReadTranslations(["translations:write", "products:write"], "category"), true);
  // The store's text includes the email footer, which is the notification settings'.
  assert.equal(canReadTranslations(["translations:read", "stores:read"], "store"), false);
  assert.equal(
    canReadTranslations(["translations:read", "stores:read", "notifications:read"], "store"),
    true,
  );
});

test("writing translations takes translations:write itself", () => {
  assert.equal(canWriteTranslations(["translations:write", "products:read"], "product"), true);
  assert.equal(canWriteTranslations(["translations:read", "products:write"], "product"), false);
  assert.equal(canWriteTranslations(["translations:write"], "product"), false);
});

test("the editor's address names the kind, the resource and the language", () => {
  assert.equal(translateHref("product", "prod_1"), "/translations/product/prod_1");
  assert.equal(
    translateHref("store", "store_1", "sr-Latn"),
    "/translations/store/store_1?language=sr-Latn",
  );
});

test("the way back is the resource's own page, or the list it lives in", () => {
  assert.equal(resourceHref("product", "prod_1"), "/products/prod_1");
  assert.equal(resourceHref("category", "pcat_1"), "/products/categories/pcat_1");
  assert.equal(resourceHref("checkout_rule", "crule_1"), "/settings/checkout/rules/crule_1");
  assert.equal(resourceHref("payment_method", "pm_1"), "/settings/payments");
  assert.equal(resourceHref("search_facet", "store_1"), "/settings/languages");
});

test("a plain key is a detail named by the key", () => {
  assert.deepEqual(describeField("title", "Mug"), {
    group: "details",
    name: { type: "plain", name: "title" },
  });
});

test("an option's name is labelled by its source name", () => {
  assert.deepEqual(describeField("options.opt_1.name", "Size"), {
    group: "options",
    name: { type: "option", source: "Size" },
  });
});

test("an option value is labelled by its option and its source label", () => {
  assert.deepEqual(describeField("option_values.optval_1.label", "Large", optionOf), {
    group: "option_values",
    name: { type: "option_value", option: "Size", source: "Large" },
  });
  // A value whose option the page doesn't know still has its own label.
  assert.deepEqual(describeField("option_values.optval_2.label", "Red", optionOf), {
    group: "option_values",
    name: { type: "option_value", option: undefined, source: "Red" },
  });
});

test("choices and facets are labelled by their source", () => {
  assert.deepEqual(describeField("choices.cotton.label", "Cotton"), {
    group: "choices",
    name: { type: "choice", source: "Cotton" },
  });
  // A facet's key has dots of its own.
  assert.deepEqual(describeField("facets.option.Color.label", "Colour"), {
    group: "facets",
    name: { type: "facet", source: "Colour" },
  });
});

test("a child without source text is named by its key", () => {
  assert.deepEqual(describeField("facets.option.Color.label", "  "), {
    group: "facets",
    name: { type: "facet", source: "option.Color" },
  });
});

test("a key this build doesn't know falls back to the raw key", () => {
  assert.deepEqual(describeField("bundles.b_1.caption", "Set of two"), {
    group: "details",
    name: { type: "plain", name: "bundles.b_1.caption" },
  });
  assert.deepEqual(describeField("options.opt_1.hint", "Pick one"), {
    group: "details",
    name: { type: "plain", name: "options.opt_1.hint" },
  });
});

test("fields are grouped in the sections' order, keeping the API's order within", () => {
  const sections = grouped([
    field("option_values.optval_1.label", "Large"),
    field("title", "Mug"),
    field("options.opt_1.name", "Size"),
    field("description", "<p>A mug.</p>"),
    field("option_values.optval_2.label", "Small"),
  ]);
  assert.deepEqual(
    sections.map((s) => [s.group, s.fields.map((f) => f.field.key)]),
    [
      ["details", ["title", "description"]],
      ["options", ["options.opt_1.name"]],
      ["option_values", ["option_values.optval_1.label", "option_values.optval_2.label"]],
    ],
  );
});

test("a draft starts from the language's saved values, blank where there are none", () => {
  const fields = [field("title", "Mug", { de: upToDate("Becher", "title") }), field("alt", "")];
  assert.deepEqual(draftOf(fields, "de"), {
    title: { value: "Becher", reviewed: false },
    alt: { value: "", reviewed: false },
  });
  assert.deepEqual(draftOf(fields, "fr"), {
    title: { value: "", reviewed: false },
    alt: { value: "", reviewed: false },
  });
});

test("an untouched draft isn't dirty and saves each field without a digest", () => {
  const fields = [
    field("title", "Mug", { de: upToDate("Becher", "title") }),
    field("description", "<p>New</p>", { de: outdated("<p>Alt</p>") }, "html"),
    field("alt", "A mug"),
  ];
  const draft = draftOf(fields, "de");
  assert.equal(isDirty(fields, "de", draft), false);
  // The outdated field keeps its old digest by sending none; the blank one is left out.
  assert.deepEqual(putBody(fields, "de", draft), {
    fields: { title: { value: "Becher" }, description: { value: "<p>Alt</p>" } },
  });
});

test("an edited field is sent with the source's digest as the translator saw it", () => {
  const fields = [
    field("title", "Mug", { de: outdated("Becher") }),
    field("alt", "A mug", { de: outdated("Ein Becher") }),
  ];
  const draft = { ...draftOf(fields, "de"), title: { value: "Tasse", reviewed: false } };
  assert.equal(isDirty(fields, "de", draft), true);
  assert.deepEqual(putBody(fields, "de", draft), {
    fields: {
      title: { value: "Tasse", source_digest: "digest-of-title" },
      alt: { value: "Ein Becher" },
    },
  });
});

test("an edit is sent with the digest of the source it was made against", () => {
  // Typed against the old source; the page then read the new one (`digest-of-title`).
  const fields = [field("title", "Mug", { de: upToDate("Becher", "title") })];
  const draft = { title: { value: "Tasse", reviewed: false, against: "digest-before" } };
  // So the API still refuses it as `source_changed` until the translator deals with it.
  assert.deepEqual(putBody(fields, "de", draft), {
    fields: { title: { value: "Tasse", source_digest: "digest-before" } },
  });
});

test("a field marked reviewed is sent with the digest though its value is the same", () => {
  const fields = [field("title", "Mug", { de: outdated("Becher") })];
  const draft = { title: { value: "Becher", reviewed: true } };
  assert.equal(isDirty(fields, "de", draft), true);
  assert.deepEqual(putBody(fields, "de", draft), {
    fields: { title: { value: "Becher", source_digest: "digest-of-title" } },
  });
});

test("marking an up-to-date translation reviewed changes nothing", () => {
  const fields = [field("title", "Mug", { de: upToDate("Becher", "title") })];
  assert.equal(isDirty(fields, "de", { title: { value: "Becher", reviewed: true } }), false);
});

test("a new translation is sent with the digest", () => {
  const fields = [field("title", "Mug")];
  const draft = { title: { value: "Becher", reviewed: false } };
  assert.equal(isDirty(fields, "de", draft), true);
  assert.deepEqual(putBody(fields, "de", draft), {
    fields: { title: { value: "Becher", source_digest: "digest-of-title" } },
  });
});

test("a cleared field is left out, which removes it", () => {
  const fields = [
    field("title", "Mug", { de: upToDate("Becher", "title") }),
    field("alt", "A mug", { de: upToDate("Ein Becher", "alt") }),
  ];
  const draft = { ...draftOf(fields, "de"), alt: { value: "   ", reviewed: false } };
  assert.equal(isDirty(fields, "de", draft), true);
  assert.deepEqual(putBody(fields, "de", draft), { fields: { title: { value: "Becher" } } });
});

test("blank over nothing saved isn't a change", () => {
  const fields = [field("title", "Mug")];
  assert.equal(isDirty(fields, "de", { title: { value: "  ", reviewed: false } }), false);
  assert.deepEqual(putBody(fields, "de", { title: { value: "  ", reviewed: false } }), {
    fields: {},
  });
});

test("another language's values never reach the body", () => {
  const fields = [field("title", "Mug", { fr: upToDate("Tasse", "title") })];
  assert.deepEqual(putBody(fields, "de", draftOf(fields, "de")), { fields: {} });
});

test("a field without source text can't be translated", () => {
  assert.equal(isTranslatable(field("alt", "")), false);
  assert.equal(isTranslatable(field("alt", " \n ")), false);
  assert.equal(isTranslatable(field("alt", "A mug")), true);
});

test("a field without source text is never sent, so the API has nothing to refuse", () => {
  // A translation kept from before the source was emptied is dropped by the save.
  const fields = [
    field("title", "Mug", { de: upToDate("Becher", "title") }),
    field("alt", "", { de: upToDate("Ein Becher", "alt") }),
  ];
  const draft = { ...draftOf(fields, "de"), title: { value: "Tasse", reviewed: false } };
  assert.deepEqual(putBody(fields, "de", draft), {
    fields: { title: { value: "Tasse", source_digest: "digest-of-title" } },
  });
});

test("a field without source text never makes the draft dirty", () => {
  const fields = [field("alt", "")];
  assert.equal(isDirty(fields, "de", { alt: { value: "Ein Becher", reviewed: true } }), false);
});

test("text is counted in characters, HTML in bytes", () => {
  assert.equal(lengthOf("Größe", "text"), 5);
  // One code point outside the BMP is one character, as the API counts.
  assert.equal(lengthOf("a😀", "text"), 2);
  assert.equal(lengthOf("<p>ö</p>", "html"), 9);
});

test("a save's problems are read by field, the first of each", () => {
  const details = {
    problems: [
      { key: "title", code: "too_long", message: "at most 255 characters" },
      { key: "title", code: "source_changed", message: "the source changed" },
      { key: "options.opt_9.name", code: "unknown_child", message: "no such option" },
      { code: "too_long" },
      "nonsense",
    ],
  };
  assert.deepEqual(problemsOf(details), {
    title: { code: "too_long", message: "at most 255 characters" },
    "options.opt_9.name": { code: "unknown_child", message: "no such option" },
  });
  assert.deepEqual(problemsOf(undefined), {});
  assert.deepEqual(problemsOf({ problems: "none" }), {});
});
