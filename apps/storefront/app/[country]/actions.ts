"use server";

// Every change a shopper makes, as server actions (design note 0023, SF5): the cart ID
// stays in an httpOnly cookie and the publishable key on the server. Each action ends by
// redirecting back to its page, with `?error=` when the API refused.

import { cookies } from "next/headers";
import { redirect } from "next/navigation";

import { api, describe } from "@/lib/api";
import { COUNTRY_COOKIE } from "@/lib/routing";
import { cartId, forgetCart, link, mode, rememberCart } from "@/lib/site";

type ApiError = { code: string; error: string };

function field(form: FormData, name: string): string {
  const value = form.get(name);
  return typeof value === "string" ? value : "";
}

/** `value` trimmed, or `null` for a blank optional field. */
function optional(form: FormData, name: string): string | null {
  const value = field(form, name).trim();
  return value === "" ? null : value;
}

function back(path: string, error?: ApiError | string): never {
  if (!error) redirect(path);
  const message = typeof error === "string" ? error : describe(error);
  const separator = path.includes("?") ? "&" : "?";
  redirect(`${path}${separator}error=${encodeURIComponent(message)}`);
}

/** The shopper's open cart in `country`: the one in the cookie, or a new one. */
async function openCart(country: string): Promise<string> {
  const client = api();
  const existing = await cartId(country);
  if (existing) {
    const { data } = await client.GET("/storefront/carts/{id}", {
      params: { path: { id: existing } },
    });
    if (data && !data.order) return existing;
  }
  const { data, error } = await client.POST("/storefront/carts", { body: { country } });
  if (error) throw new Error(`couldn't create a cart: ${error.code}`);
  await rememberCart(country, data.id);
  return data.id;
}

/** Adds a variant to the cart, then shows the cart. */
export async function addToCart(form: FormData) {
  const country = field(form, "country");
  const variant = field(form, "variant");
  const quantity = Math.max(1, Number(field(form, "quantity")) || 1);
  const from = field(form, "from");
  const id = await openCart(country);
  const { error } = await api().POST("/storefront/carts/{id}/lines", {
    params: { path: { id } },
    body: { variant, quantity },
  });
  if (error) back(from, error);
  redirect(link(country, "/cart"));
}

export async function setQuantity(form: FormData) {
  const country = field(form, "country");
  const id = await cartId(country);
  const cart = link(country, "/cart");
  if (!id) back(cart);
  const quantity = Number(field(form, "quantity"));
  const variant = field(form, "variant");
  const client = api();
  const { error } =
    quantity > 0
      ? await client.PUT("/storefront/carts/{id}/lines/{variant}", {
          params: { path: { id, variant } },
          body: { quantity },
        })
      : await client.DELETE("/storefront/carts/{id}/lines/{variant}", {
          params: { path: { id, variant } },
        });
  back(cart, error);
}

/** Checkout's first two steps: the email and the shipping address, in one form. */
export async function saveDetails(form: FormData) {
  const country = field(form, "country");
  const checkout = link(country, "/checkout");
  const id = await cartId(country);
  if (!id) back(link(country, "/cart"));
  const client = api();
  const email = await client.PUT("/storefront/carts/{id}/email", {
    params: { path: { id } },
    body: { email: field(form, "email").trim() },
  });
  if (email.error) back(checkout, email.error);
  const address = await client.PUT("/storefront/carts/{id}/shipping-address", {
    params: { path: { id } },
    body: {
      address: {
        first_name: optional(form, "first_name"),
        last_name: field(form, "last_name"),
        company: optional(form, "company"),
        line1: field(form, "line1"),
        line2: optional(form, "line2"),
        city: field(form, "city"),
        postal_code: optional(form, "postal_code"),
        province: optional(form, "province"),
        country,
        phone: optional(form, "phone"),
      },
    },
  });
  back(checkout, address.error);
}

/** Checkout's third step: a shipping option for one group of lines. */
export async function selectShipping(form: FormData) {
  const country = field(form, "country");
  const checkout = link(country, "/checkout");
  const id = await cartId(country);
  if (!id) back(link(country, "/cart"));
  const { error } = await api().PUT("/storefront/carts/{id}/shipping/{group_id}", {
    params: { path: { id, group_id: field(form, "group") } },
    body: { option: field(form, "option") },
  });
  back(checkout, error);
}

/**
 * Places the order, unpaid (0021, SU8), at the total the shopper saw: a total that moved
 * since is refused (`total_changed`) rather than charged. The idempotency key makes a
 * double click place one order.
 */
export async function placeOrder(form: FormData) {
  const country = field(form, "country");
  const checkout = link(country, "/checkout");
  const id = await cartId(country);
  if (!id) back(link(country, "/cart"));
  const { error } = await api().POST("/storefront/carts/{id}/checkout", {
    params: { path: { id }, header: { "Idempotency-Key": field(form, "attempt") } },
    body: {
      expected_total: {
        amount: Number(field(form, "total_amount")),
        currency: field(form, "total_currency"),
      },
    },
  });
  if (error) back(checkout, error);
  // The confirmation reads the order through the checked-out cart.
  (await cookies()).set(`order_${country.toLowerCase()}`, id, {
    httpOnly: true,
    sameSite: "lax",
    secure: process.env.NODE_ENV === "production",
    path: "/",
    maxAge: 60 * 60 * 24,
  });
  await forgetCart(country);
  redirect(link(country, "/order"));
}

/**
 * Switches country (the picker): remembers it, and goes to the same page there. With
 * folders, that's the page under the new country's folder.
 */
export async function setCountry(form: FormData) {
  const country = field(form, "country").toUpperCase();
  const path = field(form, "path") || "/";
  (await cookies()).set(COUNTRY_COOKIE, country, {
    sameSite: "lax",
    path: "/",
    maxAge: 60 * 60 * 24 * 365,
  });
  if (mode() === "single") redirect(path);
  // `/hr/p/mug` → `/p/mug`, then into the new folder.
  const rest = path.replace(/^\/[a-z]{2}(?=\/|$)/, "") || "/";
  redirect(link(country, rest));
}
