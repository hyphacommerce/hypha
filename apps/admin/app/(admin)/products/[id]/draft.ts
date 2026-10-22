// The product editor's draft: every edit to details, content, options and variants
// lands here first, and one Save sends it. Plain data and pure functions, shared by the
// editor's cards and the save action's input.

import type { AdminSchemas } from "@hypha/sdk/admin";

import { type Currencies, moneyInput, parseMoney } from "@/lib/format";

type Product = AdminSchemas["ProductResponse"];

/** The API refuses more; "Create all variants" stops short of it (`Product::MAX_VARIANTS`). */
export const MAX_VARIANTS = 2048;

/** An option value; `key` is stable for React and for variants' selections, `id` is the
 * saved value's (a saved value's key is its ID). */
export type ValueDraft = { key: string; id?: string; label: string };

export type OptionDraft = {
  key: string;
  id?: string;
  name: string;
  values: ValueDraft[];
  /** The "Add a value" input. */
  adding: string;
};

/** A variant as typed. `selections` maps an option's key to the key of its value; an
 * option with no entry (added in this draft) takes its first value, as the API does. */
export type VariantDraft = {
  key: string;
  id?: string;
  sku: string;
  price: string;
  weight: string;
  ships: boolean;
  selections: Record<string, string>;
};

export type Draft = {
  title: string;
  handle: string;
  description: string;
  vendor: string;
  productType: string;
  tags: string[];
  options: OptionDraft[];
  variants: VariantDraft[];
  /** The categories it's directly in, sorted. */
  categories: string[];
  /** The sales channels it's published to, sorted. */
  channels: string[];
};

let nextKey = 0;
export const newKey = () => `new-${nextKey++}`;

export function fromProduct(product: Product, currencies: Currencies): Draft {
  return {
    title: product.title,
    handle: product.handle,
    description: product.description ?? "",
    vendor: product.vendor ?? "",
    productType: product.product_type ?? "",
    tags: product.tags,
    options: product.options.map((o) => ({
      key: o.id,
      id: o.id,
      name: o.name,
      values: o.values.map((v) => ({ key: v.id, id: v.id, label: v.label })),
      adding: "",
    })),
    variants: product.variants.map((v) => ({
      key: v.id,
      id: v.id,
      sku: v.sku ?? "",
      price: moneyInput(v.price, currencies),
      weight: v.weight_grams == null ? "" : String(v.weight_grams),
      ships: v.requires_shipping,
      selections: Object.fromEntries(
        product.options.map((o, i) => [o.id, v.selections[i]] as const),
      ),
    })),
    categories: product.categories.toSorted(),
    channels: product.channels.toSorted(),
  };
}

/** An option's values, with one typed in "Add a value" but not yet added with Enter:
 * it shows in its option, so saving shouldn't drop it. */
export function valuesOf(option: OptionDraft): ValueDraft[] {
  const typed = option.adding.trim();
  return typed === ""
    ? option.values
    : [...option.values, { key: `${option.key}:typed`, label: typed }];
}

/** The value keys a variant selects, one per option in order; `null` where its value
 * was removed from the draft (the variant goes with it). An option with no values yet
 * (being typed in) selects `""`: it removes nothing, and saving refuses it. */
export function resolvedSelections(draft: Draft, variant: VariantDraft): (string | null)[] {
  return draft.options.map((option) => {
    const values = valuesOf(option);
    if (values.length === 0) return "";
    const chosen = variant.selections[option.key];
    if (chosen === undefined) return values[0].key;
    return values.some((v) => v.key === chosen) ? chosen : null;
  });
}

/** Variants that select a value the draft removed: saving removes them. */
export function orphaned(draft: Draft): VariantDraft[] {
  return draft.variants.filter((v) => resolvedSelections(draft, v).includes(null));
}

/** The variants that saving keeps, in order. */
export function kept(draft: Draft): VariantDraft[] {
  return draft.variants.filter((v) => !resolvedSelections(draft, v).includes(null));
}

/** "M / Blue", from the draft's labels; `null` without options. */
export function variantTitle(draft: Draft, variant: VariantDraft): string | null {
  if (draft.options.length === 0) return null;
  return resolvedSelections(draft, variant)
    .map((key, i) => valuesOf(draft.options[i]).find((v) => v.key === key)?.label ?? "—")
    .join(" / ");
}

/** What the options PUT sends. */
export function optionsRequest(draft: Draft): AdminSchemas["OptionRequest"][] {
  return draft.options.map((o) => ({
    id: o.id,
    name: o.name,
    values: valuesOf(o).map((v) => ({ id: v.id, label: v.label })),
  }));
}

/** Grams from what someone typed: a whole number, or null for a blank (not entered). */
export function parseGrams(input: string): number | null | undefined {
  const text = input.trim();
  if (text === "") return null;
  if (!/^\d+$/.test(text)) return undefined;
  const grams = Number(text);
  return grams <= 4_294_967_295 ? grams : undefined;
}

/** A variant's selection, named so the save can find values created by the same save:
 * by ID once saved, by option name and label until then. */
export type SelectionRef = {
  option: { id?: string; name: string };
  value: { id?: string; label: string };
};

export type VariantPlan = {
  id: string | null;
  sku: string | null;
  price: AdminSchemas["Money"];
  weight_grams: number | null;
  requires_shipping: boolean;
  selections: SelectionRef[];
};

/** What saving sends: only the parts that changed. */
export type SavePlan = {
  product?: AdminSchemas["UpdateProductRequest"];
  options?: { options: AdminSchemas["OptionRequest"][]; removeVariants: string[] };
  variants?: VariantPlan[];
  /** The product's whole set of categories (`PUT …/categories`). */
  categories?: string[];
  /** The channels it's published to, as a whole set (`PUT …/channels`). */
  channels?: string[];
};

function details(d: Draft) {
  return [d.title.trim(), d.handle];
}

function content(d: Draft) {
  return [d.description.trim(), d.vendor.trim(), d.productType.trim(), d.tags];
}

/** The variants as the API compares them: prices in minor units, text trimmed. A price
 * or weight that doesn't parse stays as typed, so it reads as changed. */
function variantsKey(draft: Draft, currency: string, currencies: Currencies) {
  return kept(draft).map((v) => ({
    id: v.id ?? null,
    sku: v.sku.trim(),
    price: parseMoney(v.price, currency, currencies) ?? v.price,
    weight: parseGrams(v.weight) ?? v.weight,
    ships: v.ships,
    selections: resolvedSelections(draft, v),
  }));
}

const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);

/** Which parts of `draft` differ from `saved`. */
export function changes(draft: Draft, saved: Draft, currency: string, currencies: Currencies) {
  const options = !same(optionsRequest(draft), optionsRequest(saved));
  return {
    product: !same(details(draft), details(saved)) || !same(content(draft), content(saved)),
    options,
    variants:
      options ||
      !same(variantsKey(draft, currency, currencies), variantsKey(saved, currency, currencies)),
    categories: !same(draft.categories.toSorted(), saved.categories.toSorted()),
    channels: !same(draft.channels.toSorted(), saved.channels.toSorted()),
  };
}

/** A cleared text field: the API clears it with `null`. */
function blankIsNull(s: string): string | null {
  return s.trim() === "" ? null : s;
}

/**
 * The save plan for `draft` against `saved`, or a message saying what to fix first. Every
 * price and weight is checked here, before anything is sent, so a typo can't leave half
 * the product saved.
 */
export function plan(
  draft: Draft,
  saved: Draft,
  currency: string,
  currencies: Currencies,
): { plan: SavePlan } | { error: string } {
  const changed = changes(draft, saved, currency, currencies);
  const result: SavePlan = {};
  if (changed.product) {
    const body: AdminSchemas["UpdateProductRequest"] = {};
    if (draft.title.trim() !== saved.title) body.title = draft.title;
    if (draft.handle !== saved.handle) body.handle = draft.handle;
    if (!same(content(draft), content(saved))) {
      body.description = blankIsNull(draft.description);
      body.vendor = blankIsNull(draft.vendor);
      body.product_type = blankIsNull(draft.productType);
      body.tags = draft.tags;
    }
    result.product = body;
  }
  const problem = optionsProblem(draft);
  if (problem) return { error: problem };
  const variants = kept(draft);
  if (variants.length === 0) return { error: "A product keeps at least one variant." };
  if (changed.options) {
    const keptIds = new Set(variants.flatMap((v) => (v.id ? [v.id] : [])));
    result.options = {
      options: optionsRequest(draft),
      // Variants the draft dropped (deleted, or using a removed value) go in the same
      // write, before the new options apply, so they can't collide with the others.
      removeVariants: saved.variants.flatMap((v) => (v.id && !keptIds.has(v.id) ? [v.id] : [])),
    };
  }
  if (changed.variants) {
    const seen = new Set<string>();
    const planned: VariantPlan[] = [];
    for (const v of variants) {
      const title = variantTitle(draft, v) ?? "the variant";
      const amount = parseMoney(v.price, currency, currencies);
      if (amount === null)
        return { error: `Enter the price of ${title} as a number, like 18 or 18.50.` };
      const grams = parseGrams(v.weight);
      if (grams === undefined)
        return { error: `Enter the weight of ${title} in whole grams, like 350.` };
      const keys = resolvedSelections(draft, v);
      const combination = keys.join("\n");
      if (seen.has(combination)) return { error: `Two variants are ${title}. Remove one of them.` };
      seen.add(combination);
      planned.push({
        id: v.id ?? null,
        sku: v.sku.trim() === "" ? null : v.sku,
        price: { amount, currency },
        weight_grams: grams,
        requires_shipping: v.ships,
        selections: draft.options.map((option, i) => {
          const value = valuesOf(option).find((x) => x.key === keys[i]);
          return {
            option: { id: option.id, name: option.name },
            value: { id: value?.id, label: value?.label ?? "" },
          };
        }),
      });
    }
    result.variants = planned;
  }
  if (changed.categories) result.categories = draft.categories.toSorted();
  if (changed.channels) result.channels = draft.channels.toSorted();
  return { plan: result };
}

/** What's wrong with the draft's options before the API sees them, or `null`. */
export function optionsProblem(draft: Draft): string | null {
  const names = new Set<string>();
  for (const option of draft.options) {
    const name = option.name.trim();
    if (name === "") return "Name every option, or remove the ones you don't need.";
    if (names.has(name.toLowerCase())) return `Two options are called ${name}.`;
    names.add(name.toLowerCase());
    const labels = valuesOf(option).map((v) => v.label.trim().toLowerCase());
    if (labels.length === 0) return `Give ${name} at least one value.`;
    if (labels.includes("")) return `A value of ${name} is blank: enter it or remove it.`;
    if (new Set(labels).size !== labels.length) return `${name} has the same value twice.`;
  }
  return null;
}

/** How many combinations of the options' values no kept variant has (0 without
 * options); `null` when there are more than a product may have. */
export function missingCombinations(draft: Draft): number | null {
  if (draft.options.length === 0) return 0;
  const lists = draft.options.map((o) => valuesOf(o).filter((v) => v.label.trim() !== ""));
  const total = lists.reduce((n, values) => n * values.length, 1);
  if (total > MAX_VARIANTS) return null;
  const existing = new Set(kept(draft).map((v) => resolvedSelections(draft, v).join("\n")));
  let combinations: string[][] = [[]];
  for (const values of lists) {
    combinations = combinations.flatMap((keys) => values.map((v) => [...keys, v.key]));
  }
  return combinations.filter((keys) => !existing.has(keys.join("\n"))).length;
}

/** The draft with each option's typed "Add a value" text added as a value, so variants
 * can select it. */
export function withTypedValues(draft: Draft): Draft {
  return {
    ...draft,
    options: draft.options.map((o) => {
      const typed = o.adding.trim();
      return typed === ""
        ? o
        : { ...o, values: [...o.values, { key: newKey(), label: typed }], adding: "" };
    }),
  };
}

/**
 * The draft with every combination of its options' values that no kept variant has
 * yet, each priced like the first variant, or a message when there are too many.
 */
export function withAllCombinations(current: Draft): { draft: Draft } | { error: string } {
  const draft = withTypedValues(current);
  if (draft.options.length === 0) return { error: "Add an option with values first." };
  const lists = draft.options.map((o) => valuesOf(o).filter((v) => v.label.trim() !== ""));
  if (lists.some((values) => values.length === 0)) {
    return { error: "Every option needs at least one value." };
  }
  const total = lists.reduce((n, values) => n * values.length, 1);
  if (total > MAX_VARIANTS) {
    return {
      error: `Those options make ${total.toLocaleString("en")} combinations; a product has at most 2,048 variants.`,
    };
  }
  const existing = new Set(kept(draft).map((v) => resolvedSelections(draft, v).join("\n")));
  const first = kept(draft)[0];
  let combinations: string[][] = [[]];
  for (const values of lists) {
    combinations = combinations.flatMap((keys) => values.map((v) => [...keys, v.key]));
  }
  const added: VariantDraft[] = combinations
    .filter((keys) => !existing.has(keys.join("\n")))
    .map((keys) => ({
      key: newKey(),
      sku: "",
      price: first?.price ?? "",
      weight: first?.weight ?? "",
      ships: first?.ships ?? true,
      selections: Object.fromEntries(draft.options.map((o, i) => [o.key, keys[i]] as const)),
    }));
  if (kept(draft).length + added.length > MAX_VARIANTS) {
    return { error: "That would make more than 2,048 variants." };
  }
  return { draft: { ...draft, variants: [...draft.variants, ...added] } };
}

/**
 * After a save stored the options but not what followed: the draft's options and
 * selections pointing at what was stored (new options and values matched by name and
 * label), so saving again doesn't create them twice.
 */
export function rebaseOptions(draft: Draft, stored: AdminSchemas["OptionResponse"][]): Draft {
  const keyChanges = new Map<string, string>();
  const options = draft.options.map((o) => {
    const match = stored.find((s) => (o.id ? s.id === o.id : s.name === o.name.trim()));
    if (!match) return o;
    keyChanges.set(o.key, match.id);
    return {
      key: match.id,
      id: match.id,
      name: match.name,
      adding: "",
      values: valuesOf(o).map((v) => {
        const value = match.values.find((s) => (v.id ? s.id === v.id : s.label === v.label.trim()));
        if (!value) return v;
        keyChanges.set(v.key, value.id);
        return { key: value.id, id: value.id, label: value.label };
      }),
    };
  });
  const variants = draft.variants.map((v) => ({
    ...v,
    selections: Object.fromEntries(
      Object.entries(v.selections).map(([o, value]) => [
        keyChanges.get(o) ?? o,
        keyChanges.get(value) ?? value,
      ]),
    ),
  }));
  return { ...draft, options, variants };
}
