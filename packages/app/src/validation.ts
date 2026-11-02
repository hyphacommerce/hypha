// `checkout.validate`'s messages: what an app says about a cart, each on the part of the
// cart it's about. Errors stop checkout; warnings never do.

import type { components } from "./generated/extensions.ts";

/** One message in a `checkout.validate` answer. */
export type ValidationMessage = components["schemas"]["ValidationMessage"];

/** The fields of an address a message can point at. */
export type AddressField =
  | "first_name"
  | "last_name"
  | "company"
  | "line1"
  | "line2"
  | "city"
  | "province"
  | "postal_code"
  | "country"
  | "phone";

/**
 * Where a message shows: a JSONPath into the cart's response, from the set Hypha takes. An
 * index must be one the request has (`cart.lines`, `cart.shipping_groups`), or Hypha
 * refuses the whole answer.
 */
export type MessagePath =
  | "$"
  | "$.email"
  | "$.shipping_address"
  | `$.shipping_address.${AddressField}`
  | `$.shipping_groups[${number}]`
  | `$.lines[${number}]`;

/** Snake_case, a letter first, at most 64 characters. */
const CODE = /^[a-z][a-z0-9_]{0,63}$/;

/** What Hypha takes in one message. */
export const MAX_MESSAGE_LENGTH = 500;

/**
 * One message for a `checkout.validate` answer: an error by default. Throws on a code or
 * text Hypha would refuse, so the mistake shows in the app rather than as a failed call.
 */
export function message(
  path: MessagePath,
  code: string,
  text: string,
  severity: ValidationMessage["severity"] = "error",
): ValidationMessage {
  if (!CODE.test(code) || code.endsWith("_")) {
    throw new TypeError(
      `${JSON.stringify(code)} is not a message code: snake_case, a letter first`,
    );
  }
  const trimmed = text.trim();
  // Counted in code points, as Hypha counts characters.
  if (trimmed.length === 0 || Array.from(trimmed).length > MAX_MESSAGE_LENGTH) {
    throw new TypeError(`a message is 1 to ${MAX_MESSAGE_LENGTH} characters`);
  }
  return { severity, code, path, message: trimmed };
}
