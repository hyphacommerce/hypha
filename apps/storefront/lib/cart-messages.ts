// A cart's messages (`messages` on every cart, `details.messages` of a `checkout_blocked`)
// placed by their path: under an address field, the address, the email, a shipping group,
// a line, or at the top for the cart and any path this build doesn't place. Safe on both
// sides: no API calls here.

import type { StorefrontSchemas } from "@hyphacommerce/sdk/storefront";

import { type AddressField, type MessageParams, asField, inSentence } from "./address.ts";
import type { TFor } from "./messages.ts";

export type CartMessage = StorefrontSchemas["CartMessageResponse"];

/** What `wordMessage` words: a cart message, or an address problem (no `source`). */
export type Wordable = {
  code: string;
  message: string;
  params?: MessageParams | null;
  source?: CartMessage["source"];
};

/** A field's label as the form around the message shows it (`fieldLabel`), or
 * `undefined` where there's no form to ask. */
export type LabelOf = (field: string) => string | undefined;

/**
 * A message in the page's language: a built-in one worded from its `code` and `params`,
 * a field it names by `labelOf`; a checkout rule's or an app's (worded by whoever wrote
 * it), an unknown code, or one missing what its wording needs keep their `message`.
 */
export function wordMessage(t: TFor<"cart">, item: Wordable, labelOf?: LabelOf): string {
  // A rule's or an app's code is theirs, even one spelled like a built-in. The one
  // built-in message with an app as its source is the app that couldn't be asked.
  if (item.source?.type === "rule") return item.message;
  if (item.source?.type === "app" && item.code !== "validation_unavailable") return item.message;
  const params = item.params ?? {};
  const label = params.field ? labelOf?.(params.field) : undefined;
  const field = label === undefined ? undefined : inSentence(label, t.language);
  switch (item.code) {
    case "required":
      if (field === undefined) break;
      // A region is chosen, the rest typed (as the API words them).
      return params.field === "province"
        ? t("cart.message_required_choice", { field })
        : t("cart.message_required", { field });
    case "too_long":
      if (field === undefined || params.max == null) break;
      return t("cart.message_too_long", { field, max: params.max });
    case "invalid_format":
      if (field === undefined) break;
      return params.example
        ? t("cart.message_invalid_format_example", { field, example: params.example })
        : t("cart.message_invalid_format", { field });
    case "unknown_region":
      if (field === undefined) break;
      return t("cart.message_unknown_region", { field });
    case "region_mismatch": {
      const region = params.region_name ?? params.region;
      if (field === undefined || !region) break;
      return t("cart.message_region_mismatch", { field, region });
    }
    case "house_number_missing":
      return t("cart.message_house_number_missing");
    case "apartment_missing":
      return t("cart.message_apartment_missing");
    case "address_corrections_suggested":
      return t("cart.message_address_corrections_suggested");
    case "address_ambiguous":
      return t("cart.message_address_ambiguous");
    case "address_undeliverable":
      return t("cart.message_address_undeliverable");
    case "validation_unavailable":
      return t("cart.message_validation_unavailable");
  }
  return item.message;
}

/** How a message is shown: an error stops checkout, anything else is a warning. */
export function tone(message: CartMessage): "error" | "warning" {
  return message.severity === "error" ? "error" : "warning";
}

/** Where a message's path points, or `undefined` for a path this build doesn't know. */
export type Place =
  | { at: "cart" }
  | { at: "email" }
  | { at: "address" }
  | { at: "field"; field: AddressField }
  | { at: "group"; index: number }
  | { at: "line"; index: number };

export function placeOf(path: string): Place | undefined {
  if (path === "$") return { at: "cart" };
  if (path === "$.email") return { at: "email" };
  if (path === "$.shipping_address") return { at: "address" };
  const field = /^\$\.shipping_address\.([a-z0-9_]+)$/.exec(path);
  if (field) {
    const known = asField(field[1]);
    return known ? { at: "field", field: known } : undefined;
  }
  const indexed = /^\$\.(shipping_groups|lines)\[(0|[1-9][0-9]*)\]$/.exec(path);
  if (indexed) {
    const index = Number(indexed[2]);
    return indexed[1] === "lines" ? { at: "line", index } : { at: "group", index };
  }
  return undefined;
}

/** A cart's messages by where they're shown. */
export type PlacedMessages = {
  /** The cart's own and any this build doesn't place: shown at the top. */
  top: CartMessage[];
  email: CartMessage[];
  /** About the address as a whole. */
  address: CartMessage[];
  fields: Partial<Record<AddressField, CartMessage[]>>;
  /** By the shipping group's index in the cart's `shipping_groups`. */
  groups: Record<number, CartMessage[]>;
  /** By the line's index in the cart's `lines`. */
  lines: Record<number, CartMessage[]>;
};

export function placeMessages(messages: readonly CartMessage[]): PlacedMessages {
  const placed: PlacedMessages = {
    top: [],
    email: [],
    address: [],
    fields: {},
    groups: {},
    lines: {},
  };
  for (const message of messages) {
    const place = placeOf(message.path);
    switch (place?.at) {
      case "email":
        placed.email.push(message);
        break;
      case "address":
        placed.address.push(message);
        break;
      case "field":
        (placed.fields[place.field] ??= []).push(message);
        break;
      case "group":
        (placed.groups[place.index] ??= []).push(message);
        break;
      case "line":
        (placed.lines[place.index] ??= []).push(message);
        break;
      default:
        placed.top.push(message);
    }
  }
  return placed;
}

/** The first of a field's messages as an input's status, errors before warnings, in the
 * page's language. */
export function statusOf(
  t: TFor<"cart">,
  messages: readonly CartMessage[] | undefined,
  labelOf?: LabelOf,
): { type: "error" | "warning"; message: string } | undefined {
  const first = messages?.find((m) => tone(m) === "error") ?? messages?.[0];
  return first ? { type: tone(first), message: wordMessage(t, first, labelOf) } : undefined;
}

/** The messages of a `checkout_blocked` refusal; none for another error. */
export function blockedBy(error: { code: string; details?: unknown }): CartMessage[] {
  if (error.code !== "checkout_blocked") return [];
  const details = error.details;
  if (typeof details !== "object" || details === null) return [];
  const list: unknown = Reflect.get(details, "messages");
  if (!Array.isArray(list)) return [];
  return list.filter(isMessage);
}

function isMessage(item: unknown): item is CartMessage {
  if (typeof item !== "object" || item === null) return false;
  return ["severity", "code", "path", "message"].every(
    (key) => typeof Reflect.get(item, key) === "string",
  );
}
