// The demo store's second content language for `scripts/seed.mjs`: German published, the
// markets selling to German-speaking countries offering it, and the seeded content
// translated from `seed-data/de.mjs`. Apart from the seed so a test can drive it with a
// stand-in for the API (`seed-translations.test.mjs`).
//
// Each resource's translation view (`GET /admin/translations/{kind}/{id}`) says which
// fields it has now, with their source text and digest; a field is matched to the German
// by the seed's stable names (a product's handle and the field's key, an option or value
// by its English text, a custom field value by its definition's key) and written with the
// digest it was read at. A resource that already has a German translation is left as it
// is, so running it again adds nothing.

import * as de from "./seed-data/de.mjs";

const { LANGUAGE } = de;

/** Countries where German is the language shoppers expect. */
const GERMAN_SPEAKING = new Set(["DE", "AT", "CH", "LI"]);

/** Every item of a paged admin list. */
async function all(admin, path) {
  const items = [];
  let cursor;
  do {
    const sep = path.includes("?") ? "&" : "?";
    const page = await admin.get(
      `${path}${sep}limit=100${cursor ? `&cursor=${encodeURIComponent(cursor)}` : ""}`,
    );
    items.push(...page.items);
    cursor = page.next_cursor;
  } while (cursor);
  return items;
}

/** The part of a child field's key between its prefix and suffix: `opt_…` in `options.opt_….name`. */
function childOf(key, prefix, suffix) {
  return key.startsWith(prefix) && key.endsWith(suffix)
    ? key.slice(prefix.length, key.length - suffix.length)
    : undefined;
}

/**
 * Adds German to the store's languages (published) and to the markets that sell to
 * German-speaking countries: first where every country speaks it, after the languages
 * already offered elsewhere, so a market like "Europe" keeps its default. What it changed,
 * and each German market's languages.
 */
export async function seedLanguages(admin) {
  const added = [];
  const store = await admin.get("/admin/store");
  if (store.default_language === LANGUAGE) return { store, added, markets: [] };
  const listed = store.languages.find((l) => l.language === LANGUAGE);
  if (!listed || !listed.published) {
    const languages = store.languages
      .filter((l) => l.language !== LANGUAGE)
      .map(({ language, published }) => ({ language, published }));
    await admin.patch("/admin/store", {
      languages: [...languages, { language: LANGUAGE, published: true }],
    });
    added.push(listed ? "German published" : "German as a published language");
  }

  const markets = [];
  for (const market of (await admin.get("/admin/markets")).items) {
    const german = market.countries.filter((c) => GERMAN_SPEAKING.has(c));
    if (german.length === 0) continue;
    let languages = market.languages;
    if (!languages.includes(LANGUAGE)) {
      const offered = languages.length > 0 ? languages : [store.default_language];
      languages =
        german.length === market.countries.length ? [LANGUAGE, ...offered] : [...offered, LANGUAGE];
      await admin.patch(`/admin/markets/${market.id}`, { languages });
      added.push(`market ${market.name}: ${languages.join(", ")}`);
    }
    markets.push({ name: market.name, countries: german, languages });
  }
  return { store, added, markets };
}

/**
 * Writes the German translation of the resource `kind`/`id` from `lookup(field)`, the
 * German for one field of its view (`undefined` when there's none). `"kept"` when it
 * already has one, `"added"`, or `"none"` when nothing differs from the source.
 */
async function translate(admin, kind, id, lookup) {
  const view = await admin.get(`/admin/translations/${kind}/${id}`);
  if (view.translations.some((t) => t.language === LANGUAGE)) return "kept";
  const fields = {};
  for (const field of view.fields) {
    if (!field.source.trim()) continue; // nothing to translate: the API refuses it
    const value = lookup(field);
    if (value === undefined || value === field.source) continue;
    fields[field.key] = { value, source_digest: field.digest };
  }
  if (Object.keys(fields).length === 0) return "none";
  await admin.put(`/admin/translations/${kind}/${id}/${LANGUAGE}`, { fields });
  return "added";
}

/** A custom field value's German: the owner's own, else the shared one for that text. */
function customValue(field, own) {
  const key = field.custom_field?.key;
  if (!key) return undefined;
  if (!field.custom_field.variant && own?.[key] !== undefined) return own[key];
  return de.VALUES[key]?.[field.source];
}

/** The German of a product's field: title, description, options, values, custom fields. */
function productField(product) {
  return (field) => {
    if (field.key === "title") return product.title;
    if (field.key === "description") return product.description;
    if (childOf(field.key, "options.", ".name")) return de.OPTION_NAMES[field.source];
    if (childOf(field.key, "option_values.", ".label")) return de.OPTION_VALUES[field.source];
    return customValue(field, product.fields);
  };
}

/** The German of a category's field: name, description, the buying guide. */
function categoryField(category) {
  return (field) => {
    if (field.key === "name") return category.name;
    if (field.key === "description") return category.description;
    return customValue(field, { "custom.buying_guide": category.guide });
  };
}

/** The German of a custom field definition's field: name, description, choice labels. */
function definitionField(definition) {
  return (field) => {
    if (field.key === "name") return definition.name;
    if (field.key === "description") return definition.description;
    const choice = childOf(field.key, "choices.", ".label");
    return choice ? definition.choices?.[choice] : undefined;
  };
}

/**
 * Translates everything the seed writes that has German in `seed-data/de.mjs`: products
 * (draft included), categories, the custom field definitions shoppers see, shipping
 * options, payment methods, discounts, checkout rules, the images' alt texts, the store's
 * description and the search filters' labels. Resources the seed didn't make (no German
 * for them) are left alone. How many of each kind it translated, by kind.
 */
export async function seedTranslations(admin, { store, log = () => {} } = {}) {
  const counts = {};
  const record = (kind, outcome) => {
    if (outcome === "added") counts[kind] = (counts[kind] ?? 0) + 1;
  };
  /** English alt text → German, from the titles and option values it was made of. */
  const alts = new Map();
  const assets = new Set();

  log("  products");
  for (const summary of await all(admin, "/admin/products")) {
    const german = de.PRODUCTS[summary.handle];
    if (!german) continue;
    const product = await admin.get(`/admin/products/${summary.id}`);
    alts.set(product.title, german.title);
    for (const option of product.options ?? []) {
      for (const value of option.values) {
        alts.set(
          `${product.title}, ${value.label}`,
          `${german.title}, ${de.OPTION_VALUES[value.label] ?? value.label}`,
        );
      }
    }
    for (const media of product.media ?? []) assets.add(media.asset_id);
    record("product", await translate(admin, "product", product.id, productField(german)));
  }

  log("  categories");
  const categories = [];
  const walk = (nodes) => {
    for (const node of nodes) {
      categories.push(node);
      walk(node.children ?? []);
    }
  };
  walk((await admin.get("/admin/categories")).items);
  for (const node of categories) {
    const german = de.CATEGORIES[node.handle];
    if (!german) continue;
    const category = await admin.get(`/admin/categories/${node.id}`);
    alts.set(category.name, german.name);
    if (category.image?.asset_id) assets.add(category.image.asset_id);
    record("category", await translate(admin, "category", category.id, categoryField(german)));
  }

  log("  custom fields");
  for (const definition of (await admin.get("/admin/custom-field-definitions")).items) {
    const german =
      definition.namespace === "custom" ? de.FIELDS[definition.owner_type]?.[definition.key] : null;
    if (!german) continue;
    record(
      "custom_field_definition",
      await translate(admin, "custom_field_definition", definition.id, definitionField(german)),
    );
  }

  log("  shipping, payments, discounts, checkout rules");
  for (const zone of (await admin.get("/admin/shipping-zones")).items) {
    for (const option of zone.options) {
      if (!de.SHIPPING_OPTIONS[option.name]) continue;
      record(
        "shipping_option",
        await translate(admin, "shipping_option", option.id, (field) =>
          field.key === "name" ? de.SHIPPING_OPTIONS[field.source] : undefined,
        ),
      );
    }
  }
  for (const method of (await admin.get("/admin/payment-methods")).items) {
    const german = de.PAYMENT_METHODS[method.name];
    if (!german) continue;
    record(
      "payment_method",
      await translate(admin, "payment_method", method.id, (field) => german[field.key]),
    );
  }
  for (const discount of await all(admin, "/admin/discounts")) {
    if (!de.DISCOUNTS[discount.title]) continue;
    record(
      "discount",
      await translate(admin, "discount", discount.id, (field) =>
        field.key === "title" ? de.DISCOUNTS[field.source] : undefined,
      ),
    );
  }
  for (const rule of (await admin.get("/admin/checkout-rules")).items) {
    const message = de.CHECKOUT_RULES[rule.name];
    if (!message) continue;
    record(
      "checkout_rule",
      await translate(admin, "checkout_rule", rule.id, (field) =>
        field.key === "message" ? message : undefined,
      ),
    );
  }

  log("  images, store, search filters");
  for (const asset of assets) {
    record(
      "asset",
      await translate(admin, "asset", asset, (field) =>
        field.key === "alt" ? alts.get(field.source) : undefined,
      ),
    );
  }
  const storeId = (store ?? (await admin.get("/admin/store"))).id;
  record("store", await translate(admin, "store", storeId, (field) => de.STORE[field.key]));
  record(
    "search_facet",
    await translate(admin, "search_facet", storeId, (field) => {
      const facet = childOf(field.key, "facets.", ".label");
      return facet ? de.FACETS[facet] : undefined;
    }),
  );
  return counts;
}

/** Where a market's German pages are: `/{country}` where it's the default, else `/de-{country}`. */
export function germanUrls(markets, base = "") {
  return markets.flatMap(({ countries, languages }) =>
    countries.map((country) => {
      const cc = country.toLowerCase();
      return languages[0] === LANGUAGE ? `${base}/${cc}` : `${base}/${LANGUAGE}-${cc}`;
    }),
  );
}

/**
 * German for the demo: the language, the markets and the translations. What it added, and
 * a line saying where shoppers see it (`null` when no market offers it).
 */
export async function seedGerman(admin, { storefrontUrl = "", log = () => {} } = {}) {
  const { store, added, markets } = await seedLanguages(admin);
  const counts = await seedTranslations(admin, { store, log });
  for (const [kind, n] of Object.entries(counts)) added.push(`German: ${n} ${kind} translation(s)`);
  if (markets.length === 0) return { added, where: null };
  const where = markets.map(({ name, countries, languages }) => {
    const urls = germanUrls([{ countries, languages }], storefrontUrl).join(", ");
    const order =
      languages[0] === LANGUAGE
        ? "German first"
        : `${languages.filter((l) => l !== LANGUAGE).join(", ")} first and German after, so its other countries keep their default`;
    return `${name} offers ${languages.join(", ")} (${order}): ${urls}`;
  });
  return {
    added,
    where: `The store now has German content. ${where.join("; ")}. The storefront's language switcher offers it too.`,
  };
}
