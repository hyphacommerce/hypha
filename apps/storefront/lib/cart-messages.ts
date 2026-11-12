// A cart's messages (`messages` on every cart, `details.messages` of a `checkout_blocked`)
// placed by their path: under an address field, the address, the email, a shipping group,
// a line, or at the top for the cart and any path this build doesn't place. Safe on both
// sides: no API calls here.

import type { StorefrontSchemas } from "@hyphacommerce/sdk/storefront";

import { type AddressField, asField } from "./address.ts";

export type CartMessage = StorefrontSchemas["CartMessageResponse"];

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

/** The first of a field's messages as an input's status, errors before warnings. */
export function statusOf(
  messages: readonly CartMessage[] | undefined,
): { type: "error" | "warning"; message: string } | undefined {
  const first = messages?.find((m) => tone(m) === "error") ?? messages?.[0];
  return first ? { type: tone(first), message: first.message } : undefined;
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
