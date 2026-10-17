// Server-only: the request's place in the site, for pages and actions.

import { cookies } from "next/headers";

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
  (await cookies()).set(cartCookie(country), id, {
    httpOnly: true,
    sameSite: "lax",
    secure: process.env.NODE_ENV === "production",
    path: "/",
    maxAge: 60 * 60 * 24 * 30,
  });
}

export async function forgetCart(country: string) {
  (await cookies()).delete(cartCookie(country));
}
