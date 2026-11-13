// "Create discount"'s starting points: each fills in the kind, its classes and the
// combinations that kind of discount usually has; staff change the rest in the form.

import {
  type DiscountDraft,
  emptyConditions,
  emptyDraft,
  emptyItems,
  emptyValue,
} from "./draft.ts";

export type TemplateKey =
  | "amount_off_products"
  | "amount_off_order"
  | "buy_x_get_y"
  | "volume"
  | "free_shipping";

/** A starting point. `discounts.template_<key>` names it, with its `…_description`. */
export type Template = {
  key: TemplateKey;
  make: () => DiscountDraft;
};

function base(change: Partial<DiscountDraft>): DiscountDraft {
  return { ...emptyDraft(), conditions: emptyConditions(), ...change };
}

export function amountOffProducts(): DiscountDraft {
  return base({
    kind: {
      type: "standard",
      product: { items: emptyItems("categories"), value: { ...emptyValue(), percent: "20" } },
      order: null,
      shipping: null,
    },
    combinesWith: { product: false, order: false, shipping: true },
  });
}

export function amountOffOrder(): DiscountDraft {
  return base({
    kind: {
      type: "standard",
      product: null,
      order: { value: { ...emptyValue(), percent: "10" } },
      shipping: null,
    },
    combinesWith: { product: false, order: false, shipping: true },
  });
}

export function buyXGetY(): DiscountDraft {
  return base({
    method: "automatic",
    kind: {
      type: "buy_x_get_y",
      buys: emptyItems("products"),
      buy: { type: "quantity", quantity: "1", amounts: {} },
      gets: emptyItems("products"),
      getQuantity: "1",
      value: { ...emptyValue(), percent: "100" },
      usesPerOrder: "1",
    },
    combinesWith: { product: false, order: false, shipping: true },
  });
}

export function volumeTiers(): DiscountDraft {
  return base({
    method: "automatic",
    kind: {
      type: "tiered",
      items: emptyItems("categories"),
      measure: "quantity",
      tiers: [
        { quantity: "3", amounts: {}, value: { ...emptyValue(), percent: "10" } },
        { quantity: "5", amounts: {}, value: { ...emptyValue(), percent: "15" } },
      ],
    },
    combinesWith: { product: false, order: false, shipping: true },
  });
}

export function freeShipping(): DiscountDraft {
  return base({
    method: "automatic",
    // Stored content: the title buyers see, in the store's language rather than the
    // staff member's, so it isn't a message.
    title: "Free shipping",
    kind: {
      type: "standard",
      product: null,
      order: null,
      shipping: { value: { ...emptyValue(), percent: "100" }, maxAmount: {} },
    },
    conditions: { ...emptyConditions(), minSubtotal: { enabled: true, amounts: {} } },
    combinesWith: { product: true, order: true, shipping: false },
  });
}

export const TEMPLATES: readonly Template[] = [
  { key: "amount_off_products", make: amountOffProducts },
  { key: "amount_off_order", make: amountOffOrder },
  { key: "buy_x_get_y", make: buyXGetY },
  { key: "volume", make: volumeTiers },
  { key: "free_shipping", make: freeShipping },
];

export function templateOf(key: string): Template | undefined {
  return TEMPLATES.find((t) => t.key === key);
}
