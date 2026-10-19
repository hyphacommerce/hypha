// Server-only: the request's place in the site, for pages and actions.

import { cookies } from "next/headers";

import type { StorefrontSchemas } from "@hypha/sdk/storefront";

import { cartFlagCookie } from "./cart-flag";
import { href, routing } from "./routing";

/** The routing mode this deployment uses (`STOREFRONT_ROUTING`). */
export function mode() {
  return routing(process.env.STOREFRONT_ROUTING);
}

/** A link to `path` in `country`, for this deployment's routing. */
export function link(country: string, path: string): string {
  return href(mode(), country, path);
}

/** The cart's cookie: one cart per country, since a cart's country sets its market. */
function cartCookie(country: string): string {
  return `cart_${country.toLowerCase()}`;
}

/** The shopper's cart in `country`, if they have one. */
export async function cartId(country: string): Promise<string | undefined> {
  return (await cookies()).get(cartCookie(country))?.value;
}

export async function rememberCart(country: string, id: string) {
  const jar = await cookies();
  const options = {
    sameSite: "lax" as const,
    secure: process.env.NODE_ENV === "production",
    path: "/",
    maxAge: 60 * 60 * 24 * 30,
  };
  jar.set(cartCookie(country), id, { ...options, httpOnly: true });
  jar.set(cartFlagCookie(country), "1", options);
}

/** The cookie holding an order's access token, for its confirmation page. */
export function orderTokenCookie(order: string): string {
  return `order_token_${order}`;
}

/**
 * Keeps an order's access token for this buyer (httpOnly: the page reads it on the
 * server), from checkout or from the confirmation email's link (`app/order/[id]`).
 */
export async function rememberOrder(order: string, token: string) {
  (await cookies()).set(orderTokenCookie(order), token, {
    httpOnly: true,
    sameSite: "lax",
    secure: process.env.NODE_ENV === "production",
    path: "/",
    maxAge: 60 * 60 * 24 * 30,
  });
}

/**
 * How checkout's payment started, when the confirmation page must finish it (design
 * note 0029, §7: a payment app's `client_widget`, or a start that failed), kept for ten
 * minutes so the page mounts the widget without starting the payment again. Too large
 * for a cookie (an app's props over ~3 KB): not kept, and the page starts it again (the
 * app sees the same idempotency key).
 */
export type PaymentStart = {
  next_action: StorefrontSchemas["NextActionResponse"] | null;
  /** The shopper's message for a start that failed (`payment_error`). */
  error: string | null;
};

function paymentStartCookie(order: string): string {
  return `order_pay_${order}`;
}

export async function rememberPaymentStart(order: string, start: PaymentStart) {
  const value = JSON.stringify(start);
  if (value.length > 3000) return;
  (await cookies()).set(paymentStartCookie(order), value, {
    httpOnly: true,
    sameSite: "lax",
    secure: process.env.NODE_ENV === "production",
    path: "/",
    maxAge: 10 * 60,
  });
}

/** The kept start of `order`'s payment, if any. */
export async function paymentStartOf(order: string): Promise<PaymentStart | null> {
  const raw = (await cookies()).get(paymentStartCookie(order))?.value;
  if (!raw) return null;
  try {
    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed !== "object" || parsed === null || !("next_action" in parsed)) return null;
    const error: unknown = Reflect.get(parsed, "error");
    const next: unknown = Reflect.get(parsed, "next_action");
    return {
      next_action: isNextAction(next) ? next : null,
      error: typeof error === "string" ? error : null,
    };
  } catch {
    return null;
  }
}

function isNextAction(value: unknown): value is StorefrontSchemas["NextActionResponse"] {
  if (typeof value !== "object" || value === null) return false;
  const type: unknown = Reflect.get(value, "type");
  return type === "client_widget" || type === "redirect" || type === "instructions";
}

export async function forgetPaymentStart(order: string) {
  (await cookies()).delete(paymentStartCookie(order));
}

export async function forgetCart(country: string) {
  const jar = await cookies();
  jar.delete(cartCookie(country));
  jar.delete(cartFlagCookie(country));
}
