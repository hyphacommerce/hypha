// Server-only: the request's place in the site, for pages and actions.

import { cookies } from "next/headers";

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

export async function forgetCart(country: string) {
  const jar = await cookies();
  jar.delete(cartCookie(country));
  jar.delete(cartFlagCookie(country));
}

/** The storefront's public origin (`STOREFRONT_URL`, `https://shop.example.com`), for
 * canonical links, link previews and structured data; `undefined` when unset (they're
 * left out or relative then). Static pages can't read the request's host. */
export function siteUrl(): URL | undefined {
  const value = process.env.STOREFRONT_URL;
  if (!value) return undefined;
  try {
    return new URL(value);
  } catch {
    return undefined;
  }
}

/** `path` on the storefront's origin, or the path itself without one. */
export function absolute(path: string): string {
  const site = siteUrl();
  return site ? new URL(path, site).href : path;
}
