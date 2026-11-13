// Server-only: how the storefront talks to the Hypha storefront API. The publishable
// key stays here; pages render on the server and changes go through server actions, so
// the browser never needs it. Never import this from a "use client" file.

import { apiFetch } from "#api-fetch";
import { type StorefrontSchemas, createStorefrontClient } from "@hyphacommerce/sdk/storefront";
import { unstable_cache } from "next/cache";
import { headers } from "next/headers";
import { notFound } from "next/navigation";
import { cache } from "react";

import { type AddressFormat, type FieldProblems, problemsOf } from "./address";
import { FRESH_SECONDS, TAGS } from "./cache-tags";
import { type FieldContext, type FieldPart, referenceIds } from "./custom-fields";
import { type T as Translator, isKey } from "./messages";
import { link, segmentCountry } from "./site";

export type Country = StorefrontSchemas["StorefrontCountryResponse"];

function env(name: string): string {
  const value = process.env[name];
  if (!value) throw new Error(`${name} is not set (see apps/storefront/.env.example)`);
  return value;
}

/** A client with the storefront's publishable key (through the API's Service Binding
 * on Workers). */
export function api(extra: Record<string, string> = {}) {
  return createStorefrontClient(env("HYPHA_API_URL"), env("STOREFRONT_KEY"), {
    fetch: apiFetch(),
    headers: extra,
  });
}

/**
 * A client that passes the shopper's address on, for the calls Hypha limits per address
 * (creating a cart, entering a discount code), so the limit counts the shopper, not this server. Only from a source
 * the browser can't forge, as the admin's (`apps/admin/lib/hypha.ts`): Cloudflare's
 * `cf-connecting-ip` on Workers (the API believes it through its own Service Binding),
 * or, behind this deployment's own proxy (`STOREFRONT_TRUST_X_FORWARDED_FOR=true`, a
 * VPS's Caddy), the last `x-forwarded-for` entry, which the API believes from its trusted
 * proxies. Server actions only: it reads the request's headers.
 */
export async function shopperApi(extra: Record<string, string> = {}) {
  const h = await headers();
  let ip = h.get("cf-connecting-ip")?.trim();
  if (!ip && process.env.STOREFRONT_TRUST_X_FORWARDED_FOR === "true") {
    ip = h.get("x-forwarded-for")?.split(",").at(-1)?.trim();
  }
  return api(ip ? { ...extra, "x-forwarded-for": ip } : extra);
}

/**
 * The data of a read a page needs. A missing resource goes to the 404 page; anything
 * else is a bug or an outage, so it throws to the error page.
 */
export function loaded<T>(result: { data?: T; response: Response }): T {
  if (result.data !== undefined) return result.data;
  if (result.response.status === 404) notFound();
  throw new Error(`${result.response.url} failed with ${result.response.status}`);
}

/**
 * A read whose resource may be missing: its data, `null` when the API says 404, and a
 * throw for anything else. A failing API (a 500, a timeout) must not read as "missing":
 * the cached reads below would keep that `null`, and the static page its 404, for
 * minutes after the API is back. Thrown, nothing is cached, and the next request asks
 * again.
 */
export function found<T>(result: { data?: T; response: Response }): T | null {
  if (result.data !== undefined) return result.data;
  if (result.response.status === 404) return null;
  throw new Error(`${result.response.url} failed with ${result.response.status}`);
}

/** The store's public details, once per request (and cached, tagged `store`). */
export const loadStore = cache(
  unstable_cache(async () => loaded(await api().GET("/storefront/store")), ["store"], {
    tags: [TAGS.store],
    revalidate: FRESH_SECONDS,
  }),
);

/** Every country a market sells to, with its currency and minor units. */
export const loadCountries = cache(
  unstable_cache(
    async (): Promise<Country[]> => loaded(await api().GET("/storefront/countries")).items,
    ["countries"],
    { tags: [TAGS.store, TAGS.prices], revalidate: FRESH_SECONDS },
  ),
);

/** The country of a URL folder (`hr`), or 404 when no market sells there. */
export async function countryOf(folder: string): Promise<Country> {
  const code = segmentCountry(folder);
  const country = (await loadCountries()).find((c) => c.country === code);
  if (!country) notFound();
  return country;
}

/** A country's address form: its fields change only with a release, its `autocomplete`
 * with the store's settings, so it's read again as often as other cached reads. */
export const loadAddressFormat = cache((country: string) =>
  unstable_cache(
    async (): Promise<AddressFormat> =>
      loaded(
        await api().GET("/storefront/address-formats/{country}", {
          params: { path: { country } },
        }),
      ),
    ["address-format", country],
    { revalidate: FRESH_SECONDS },
  )(),
);

/** The shown category tree, once per request (and cached, tagged `categories`). */
export const loadCategories = cache(
  unstable_cache(
    async () => loaded(await api().GET("/storefront/categories")).items,
    ["categories"],
    {
      tags: [TAGS.categories],
      revalidate: FRESH_SECONDS,
    },
  ),
);

/**
 * A category page's category, cached and tagged with its handle, once per request (its
 * metadata and the page share it). A missing or hidden one is `null` (the page 404s),
 * not cached as an error.
 */
export const loadCategory = cache((handle: string) =>
  unstable_cache(
    async () => {
      return found(
        await api().GET("/storefront/categories/by-handle/{handle}", {
          params: { path: { handle } },
        }),
      );
    },
    ["category", handle],
    { tags: [TAGS.categories, TAGS.category(handle)], revalidate: FRESH_SECONDS },
  )(),
);

/**
 * A product page's product, priced and stocked in `country`, cached and tagged with its
 * handle (the webhook expires it), once per request (its metadata and the page share
 * it). A missing one is `null`.
 */
export const loadProduct = cache((handle: string, country: string) =>
  unstable_cache(
    async () => {
      return found(
        await api().GET("/storefront/products/by-handle/{handle}", {
          params: { path: { handle }, query: { country } },
        }),
      );
    },
    ["product", handle, country],
    { tags: [TAGS.products, TAGS.product(handle), TAGS.prices], revalidate: FRESH_SECONDS },
  )(),
);

/**
 * The custom fields storefronts are given, to name, order and label values: once per
 * request, cached and tagged `custom-fields` (the webhook expires it when a definition
 * changes).
 */
export const loadFieldDefinitions = cache(
  unstable_cache(
    async () => loaded(await api().GET("/storefront/custom-field-definitions")).items,
    ["custom-field-definitions"],
    { tags: [TAGS.fields], revalidate: FRESH_SECONDS },
  ),
);

/** References resolved one by one on a page; more aren't shown as links. */
const MAX_REFERENCES = 10;

/** A referenced product's handle and title, cached (tagged `products`, so any product's
 * change refreshes it); `null` when the storefront can't show it. */
const loadReferencedProduct = (id: string, country: string) =>
  unstable_cache(
    async () => {
      const product = found(
        await api().GET("/storefront/products/{id}", {
          params: { path: { id }, query: { country } },
        }),
      );
      return product ? { title: product.title, handle: product.handle } : null;
    },
    ["product-ref", id, country],
    { tags: [TAGS.products], revalidate: FRESH_SECONDS },
  )();

type CategoryNode = { id: string; name: string; handle: string; children: CategoryNode[] };

function flatten(nodes: CategoryNode[]): CategoryNode[] {
  return nodes.flatMap((n) => [n, ...flatten(n.children)]);
}

/**
 * What custom fields need to be shown on a page: the page's locale and words (`t`), the
 * country's currency, and the product and category references among `values` as links to their
 * pages (categories from the shown tree; products read by ID, at most `MAX_REFERENCES`).
 * A reference that can't be read isn't shown: a field is decoration, so its failure
 * never fails the page.
 */
export async function fieldContext(
  country: Country,
  t: Translator,
  ...values: (Record<string, unknown> | undefined)[]
): Promise<FieldContext> {
  const ids = referenceIds(...values.filter((v) => v !== undefined));
  const code = country.country;
  const refs: Record<string, FieldPart> = {};
  const [categories, definitions] = await Promise.all([
    ids.some((id) => id.startsWith("pcat_")) ? loadCategories() : Promise.resolve([]),
    loadFieldDefinitions(),
  ]);
  for (const c of flatten(categories)) {
    if (ids.includes(c.id)) refs[c.id] = { text: c.name, href: link(code, `/c/${c.handle}`) };
  }
  const products = ids.filter((id) => id.startsWith("prod_")).slice(0, MAX_REFERENCES);
  const read = await Promise.all(
    products.map((id) => loadReferencedProduct(id, code).catch(() => null)),
  );
  products.forEach((id, i) => {
    const product = read[i];
    if (product) refs[id] = { text: product.title, href: link(code, `/p/${product.handle}`) };
  });
  return {
    locale: t.locale,
    words: { yes: t("common.yes"), no: t("common.no") },
    currency: country.currency,
    digits: country.minor_units,
    refs,
    definitions: Object.fromEntries(
      definitions.map((d) => [
        `${d.owner_type}:${d.field}`,
        {
          name: d.name,
          position: d.position,
          type: d.type,
          choices: Object.fromEntries((d.choices ?? []).map((c) => [c.key, c.label])),
        },
      ]),
    ),
  };
}

/**
 * An action's refusal: the message to show and, for an address, each field's problem
 * (the banner then only points at them).
 */
export function refusedWith(
  t: Translator,
  error: { code: string; error: string; details?: unknown },
): {
  error: string;
  problems?: FieldProblems;
} {
  const problems = problemsOf(error);
  if (problems && Object.keys(problems).length > 0) {
    return { error: t("errors.check_fields"), problems };
  }
  return { error: describe(t, error) };
}

/** The codes that share another's sentence, by the key it's under: a code's own
 * sentence is `errors.<code>`, which needs no entry here. */
const SHARED_ERRORS = new Map([
  ["product_not_published", "product_unavailable"],
  ["product_not_active", "product_unavailable"],
  ["product_not_found", "product_unavailable"],
  ["account_required", "sign_in_again"],
  ["invalid_token", "sign_in_again"],
  ["field_required", "invalid_address"],
  ["cart_email_missing", "email_required"],
  ["shipping_option_unavailable", "shipping_option_required"],
  ["payment_method_not_found", "payment_method_unavailable"],
]);

/**
 * A message for the shopper, by the API's stable error code: `errors.<code>` in the
 * catalogs, so a code gets its sentence by being added there. The API's own `error` text
 * is for developers and always English: for a code without a sentence, it's shown as it
 * is only on an English page, and a general sentence stands in elsewhere.
 */
export function describe(t: Translator, error: { code: string; error: string }): string {
  const key = `errors.${SHARED_ERRORS.get(error.code) ?? error.code}`;
  if (isKey(key)) return t(key);
  return isEnglish(t.language) ? error.error : t("errors.generic");
}

/** Whether a page in `language` reads English (the pseudo-language included). */
function isEnglish(language: string): boolean {
  return /^en(-|$)/i.test(language);
}
