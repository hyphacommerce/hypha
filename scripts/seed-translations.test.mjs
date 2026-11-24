// `node --test scripts/seed-translations.test.mjs`: the seed's German step against a
// stand-in for the admin API (a store with one product and one category), recording what
// it would send.

import assert from "node:assert/strict";
import { test } from "node:test";

import { germanUrls, seedGerman } from "./seed-translations.mjs";

const digest = (text) => `sha-${text.length}`;
/** One field of a translation view, translated into nothing yet. */
const field = (key, source, extra = {}) => ({
  key,
  format: "text",
  max_length: 255,
  source,
  digest: digest(source),
  translations: {},
  custom_field: null,
  ...extra,
});
const view = (kind, id, fields, translations = []) => ({
  resource: { kind, id },
  default_language: "en",
  languages: ["de"],
  fields,
  translations,
});

/** The stand-in: GETs answered from `routes`, every write recorded. */
function api(routes) {
  const sent = [];
  const answer = (method) => async (path, body) => {
    if (method !== "GET") {
      sent.push({ method, path, body });
      return {};
    }
    const route = routes[path.replace(/[?&]limit=100$/, "")];
    if (route === undefined) throw new Error(`unexpected GET ${path}`);
    return typeof route === "function" ? route() : route;
  };
  return {
    sent,
    get: answer("GET"),
    put: answer("PUT"),
    patch: answer("PATCH"),
    post: answer("POST"),
  };
}

const MARKETS = [
  {
    id: "mkt_eu",
    name: "Europe",
    countries: ["HR", "AT", "DE", "FR"],
    languages: [],
  },
  { id: "mkt_dach", name: "DACH", countries: ["DE", "AT", "CH"], languages: [] },
  { id: "mkt_us", name: "United States", countries: ["US"], languages: [] },
];

function store({ productTranslations = [] } = {}) {
  return api({
    "/admin/store": {
      id: "store_1",
      default_language: "en",
      languages: [{ language: "fr", published: false }],
    },
    "/admin/markets": { items: MARKETS },
    "/admin/products": {
      items: [
        { id: "prod_1", handle: "oyster-grow-kit", title: "Oyster Mushroom Grow Kit" },
        { id: "prod_2", handle: "not-seeded", title: "Someone else's" },
      ],
    },
    "/admin/products/prod_1": {
      id: "prod_1",
      title: "Oyster Mushroom Grow Kit",
      options: [
        {
          name: "Species",
          values: [{ label: "Pearl oyster" }, { label: "Blue oyster" }],
        },
        { name: "Size", values: [{ label: "Starter" }, { label: "Family" }] },
      ],
      media: [{ asset_id: "asset_1" }, { asset_id: "asset_2" }],
    },
    "/admin/translations/product/prod_1": view(
      "product",
      "prod_1",
      [
        field("title", "Oyster Mushroom Grow Kit"),
        field("description", "<p>A block of hardwood…</p>", { format: "html" }),
        field("options.opt_1.name", "Species"),
        field("options.opt_2.name", "Size"),
        field("option_values.optval_1.label", "Pearl oyster"),
        field("option_values.optval_2.label", "Starter"), // the same in German: itself
        field("option_values.optval_3.label", "A value someone added"), // no German: left out
        field("custom_fields.cfdef_1.value", "Our lab and farm", {
          custom_field: { key: "custom.grown_in", definition: "cfdef_1", variant: null },
        }),
        field("custom_fields.cfdef_2.value", "<ol><li>Cut an X…</li></ol>", {
          format: "html",
          custom_field: { key: "custom.care_guide", definition: "cfdef_2", variant: null },
        }),
      ],
      productTranslations,
    ),
    "/admin/categories": {
      items: [{ id: "pcat_1", handle: "grow", children: [{ id: "pcat_2", handle: "grow-kits" }] }],
    },
    "/admin/categories/pcat_1": {
      id: "pcat_1",
      name: "Grow",
      image: { asset_id: "asset_3" },
    },
    "/admin/categories/pcat_2": { id: "pcat_2", name: "Grow kits", image: null },
    "/admin/translations/category/pcat_1": view("category", "pcat_1", [
      field("name", "Grow"),
      field("description", "<p>Grow from Mycelium Works.</p>", { format: "html" }),
      field("custom_fields.cfdef_9.value", "<p>New to growing?…</p>", {
        format: "html",
        custom_field: { key: "custom.buying_guide", definition: "cfdef_9", variant: null },
      }),
    ]),
    // A child without a description: an empty source isn't translated.
    "/admin/translations/category/pcat_2": view("category", "pcat_2", [
      field("name", "Grow kits"),
      field("description", ""),
    ]),
    "/admin/custom-field-definitions": { items: [] },
    "/admin/shipping-zones": {
      items: [
        {
          options: [
            { id: "ship_1", name: "Standard" },
            { id: "ship_2", name: "Royal Mail Tracked" },
            { id: "ship_3", name: "Someone's courier" },
          ],
        },
      ],
    },
    "/admin/translations/shipping_option/ship_1": view("shipping_option", "ship_1", [
      field("name", "Standard"),
    ]),
    "/admin/translations/shipping_option/ship_2": view("shipping_option", "ship_2", [
      field("name", "Royal Mail Tracked"),
    ]),
    "/admin/payment-methods": { items: [] },
    "/admin/discounts": { items: [] },
    "/admin/checkout-rules": { items: [] },
    "/admin/translations/asset/asset_1": view("asset", "asset_1", [
      field("alt", "Oyster Mushroom Grow Kit, Blue oyster"),
    ]),
    "/admin/translations/asset/asset_2": view("asset", "asset_2", [
      field("alt", "Oyster Mushroom Grow Kit, Pearl oyster"),
    ]),
    "/admin/translations/asset/asset_3": view("asset", "asset_3", [field("alt", "Grow")]),
    "/admin/translations/store/store_1": view("store", "store_1", [
      field("description", "Grow kits, lab cultures…"),
      field("email_footer", "Mycelium Works Ltd. · hello@myceliumworks.example"),
    ]),
    "/admin/translations/search_facet/store_1": view("search_facet", "store_1", [
      field("facets.field.custom.species.label", "Species"),
      field("facets.vendor.label", "Maker"),
    ]),
  });
}

const put = (sent, path) => sent.find((s) => s.method === "PUT" && s.path === path)?.body;

test("German is published beside the store's languages", async () => {
  const admin = store();
  await seedGerman(admin);
  assert.deepEqual(admin.sent[0], {
    method: "PATCH",
    path: "/admin/store",
    body: {
      languages: [
        { language: "fr", published: false },
        { language: "de", published: true },
      ],
    },
  });
});

test("a mixed market keeps its default; an all-German one leads with German", async () => {
  const admin = store();
  const { where } = await seedGerman(admin, { storefrontUrl: "http://shop" });
  const markets = admin.sent.filter((s) => s.path.startsWith("/admin/markets/"));
  assert.deepEqual(markets, [
    { method: "PATCH", path: "/admin/markets/mkt_eu", body: { languages: ["en", "de"] } },
    { method: "PATCH", path: "/admin/markets/mkt_dach", body: { languages: ["de", "en"] } },
  ]);
  assert.match(where, /http:\/\/shop\/de-at, http:\/\/shop\/de-de/);
  assert.match(where, /http:\/\/shop\/de, http:\/\/shop\/at, http:\/\/shop\/ch/);
});

test("a product's fields are matched by key and source text, with their digests", async () => {
  const admin = store();
  await seedGerman(admin);
  const { fields } = put(admin.sent, "/admin/translations/product/prod_1/de");
  // Every field the view has but one with no German.
  assert.deepEqual(Object.keys(fields), [
    "title",
    "description",
    "options.opt_1.name",
    "options.opt_2.name",
    "option_values.optval_1.label",
    "option_values.optval_2.label",
    "custom_fields.cfdef_1.value",
    "custom_fields.cfdef_2.value",
  ]);
  // A value that reads the same in German is its own translation, so it isn't missing.
  assert.deepEqual(fields["option_values.optval_2.label"], {
    value: "Starter",
    source_digest: digest("Starter"),
  });
  assert.deepEqual(fields.title, {
    value: "Austernseitling-Zuchtset",
    source_digest: digest("Oyster Mushroom Grow Kit"),
  });
  assert.deepEqual(fields["options.opt_1.name"], {
    value: "Sorte",
    source_digest: digest("Species"),
  });
  assert.deepEqual(fields["option_values.optval_1.label"], {
    value: "Austernseitling",
    source_digest: digest("Pearl oyster"),
  });
  assert.deepEqual(fields["custom_fields.cfdef_1.value"], {
    value: "Unser Labor und Hof",
    source_digest: digest("Our lab and farm"),
  });
  assert.equal(fields.description.source_digest, digest("<p>A block of hardwood…</p>"));
  assert.match(fields.description.value, /^<p>Ein Block aus Hartholz/);
  assert.match(fields["custom_fields.cfdef_2.value"].value, /^<ol><li>Schneide ein X/);
  // A product the seed didn't make is left alone.
  assert.equal(
    admin.sent.some((s) => s.path.includes("prod_2")),
    false,
  );
});

test("a category's name, description and buying guide; an empty source is skipped", async () => {
  const admin = store();
  await seedGerman(admin);
  const body = put(admin.sent, "/admin/translations/category/pcat_1/de");
  assert.deepEqual(Object.keys(body.fields), [
    "name",
    "description",
    "custom_fields.cfdef_9.value",
  ]);
  assert.deepEqual(body.fields.name, { value: "Zucht", source_digest: digest("Grow") });
  assert.match(body.fields["custom_fields.cfdef_9.value"].value, /^<p>Neu in der Pilzzucht\?/);
  assert.deepEqual(put(admin.sent, "/admin/translations/category/pcat_2/de"), {
    fields: { name: { value: "Zuchtsets", source_digest: digest("Grow kits") } },
  });
});

test("alt texts, the store and the search filters", async () => {
  const admin = store();
  await seedGerman(admin);
  const alt = (id) => put(admin.sent, `/admin/translations/asset/${id}/de`).fields.alt.value;
  assert.equal(alt("asset_1"), "Austernseitling-Zuchtset, Blauer Austernseitling");
  assert.equal(alt("asset_2"), "Austernseitling-Zuchtset, Austernseitling");
  assert.equal(alt("asset_3"), "Zucht");
  const storeFields = put(admin.sent, "/admin/translations/store/store_1/de").fields;
  assert.deepEqual(Object.keys(storeFields), ["description", "email_footer"]);
  // The email footer reads the same in German.
  assert.deepEqual(storeFields.email_footer, {
    value: "Mycelium Works Ltd. · hello@myceliumworks.example",
    source_digest: digest("Mycelium Works Ltd. · hello@myceliumworks.example"),
  });
  assert.deepEqual(put(admin.sent, "/admin/translations/search_facet/store_1/de").fields, {
    "facets.field.custom.species.label": { value: "Sorte", source_digest: digest("Species") },
    "facets.vendor.label": { value: "Hersteller", source_digest: digest("Maker") },
  });
});

test("a carrier's own name is recorded as it is; an option the seed doesn't know isn't", async () => {
  const admin = store();
  await seedGerman(admin);
  assert.deepEqual(put(admin.sent, "/admin/translations/shipping_option/ship_1/de").fields, {
    name: { value: "Standardversand", source_digest: digest("Standard") },
  });
  assert.deepEqual(put(admin.sent, "/admin/translations/shipping_option/ship_2/de").fields, {
    name: { value: "Royal Mail Tracked", source_digest: digest("Royal Mail Tracked") },
  });
  assert.equal(
    admin.sent.some((s) => s.path.includes("ship_3")),
    false,
  );
});

test("a resource with a German translation already is left as it is", async () => {
  const admin = store({
    productTranslations: [
      { language: "de", translated: 1, outdated: 0, created_at: "", updated_at: "" },
    ],
  });
  await seedGerman(admin);
  assert.equal(put(admin.sent, "/admin/translations/product/prod_1/de"), undefined);
});

test("German URLs follow the market's order", () => {
  assert.deepEqual(germanUrls([{ countries: ["AT"], languages: ["de", "en"] }]), ["/at"]);
  assert.deepEqual(germanUrls([{ countries: ["AT"], languages: ["en", "de"] }]), ["/de-at"]);
});
