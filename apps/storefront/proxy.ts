// Routing by country and language (`lib/routing.ts`'s `routeFor`). With folders (the
// default), a path that doesn't start with a place the store sells to (`/de`, `/en-de`)
// is redirected to the shopper's country, in the language they picked (the `language`
// cookie) or their browser asks for (`Accept-Language`) when the country offers it, else
// the country's default; a place naming a language its country doesn't offer goes to the
// one it comes to. In a single tree (`STOREFRONT_ROUTING=single`), every page request is
// rewritten to that folder internally, so URLs carry no country, only a language other
// than the default (`/en/…`). The country comes from the `country` cookie, then the
// host's geolocation header, then the store's first. A URL that names a language is
// never redirected because of a cookie or a header.

import { type NextRequest, NextResponse } from "next/server";

import { SESSION_COOKIE, SESSION_LIFETIME_COOKIE, sessionCookie } from "@/lib/account";
import { api } from "@/lib/api";
import { cartFlagCookie } from "@/lib/cart-flag";
import { COUNTRY_COOKIE, LANGUAGE_COOKIE, type Offer, routeFor, routing } from "@/lib/routing";

/** The store's countries, kept for a minute: they change rarely, and every page asks. */
let cached: { countries: Offer[]; until: number } | undefined;

/**
 * The store's countries with their languages. Only an answer is kept: a failed call (the
 * API restarting, a timeout) would otherwise leave every path without a country
 * unroutable for a minute. It falls back to the last answer, if there was one.
 */
async function storeCountries(): Promise<Offer[]> {
  const now = Date.now();
  if (cached && cached.until > now) return cached.countries;
  try {
    const { data } = await api().GET("/storefront/countries");
    if (data) {
      const countries = data.items.map((c) => ({ country: c.country, languages: c.languages }));
      cached = { countries, until: now + 60_000 };
      return countries;
    }
  } catch {
    // Unreachable: as a failed answer, below.
  }
  return cached?.countries ?? [];
}

/**
 * Carts remembered before the flag cookie existed (`lib/cart-flag.ts`): the flag, so the
 * page reads them. Only the cookies' names: nothing is called.
 */
function flagCarts(request: NextRequest, response: NextResponse): NextResponse {
  for (const { name } of request.cookies.getAll()) {
    const country = /^cart_([a-z]{2})$/.exec(name)?.[1];
    if (country && !request.cookies.has(cartFlagCookie(country))) {
      response.cookies.set(cartFlagCookie(country), "1", {
        sameSite: "lax",
        secure: process.env.NODE_ENV === "production",
        path: "/",
        maxAge: 60 * 60 * 24 * 30,
      });
    }
  }
  return response;
}

/**
 * The account session's cookies (`lib/account.ts`) set again with their lifetime, so
 * they last as long after this visit as the session does after its last use: the API
 * pushes the session's end out while it's used, and the cookie follows. A session that
 * ended meanwhile is dropped where it's first found invalid (`loadAccount`, `newCart`).
 */
function slideSession(request: NextRequest, response: NextResponse): NextResponse {
  const token = request.cookies.get(SESSION_COOKIE)?.value;
  const lifetime = Number(request.cookies.get(SESSION_LIFETIME_COOKIE)?.value);
  if (!token || !Number.isInteger(lifetime) || lifetime <= 0) return response;
  response.cookies.set(SESSION_COOKIE, token, { ...sessionCookie(lifetime), httpOnly: true });
  response.cookies.set(SESSION_LIFETIME_COOKIE, String(lifetime), sessionCookie(lifetime));
  return response;
}

export async function proxy(request: NextRequest) {
  return slideSession(request, flagCarts(request, await route(request)));
}

/** An email's order link (`app/order/[id]/route.ts`): country-less in both modes. */
const ORDER_LINK = /^\/order\/[^/]+$/;

async function route(request: NextRequest): Promise<NextResponse> {
  const { pathname, search } = request.nextUrl;
  if (ORDER_LINK.test(pathname)) return NextResponse.next();
  const next = routeFor({
    mode: routing(process.env.STOREFRONT_ROUTING),
    pathname,
    countries: await storeCountries(),
    countryCookie: request.cookies.get(COUNTRY_COOKIE)?.value,
    geolocated: request.headers.get("x-vercel-ip-country") ?? request.headers.get("cf-ipcountry"),
    languageCookie: request.cookies.get(LANGUAGE_COOKIE)?.value,
    acceptLanguage: request.headers.get("accept-language"),
  });
  switch (next.kind) {
    // No market sells anywhere yet (the pages say so), or the path is a place's already.
    case "next":
      return NextResponse.next();
    case "rewrite": {
      const url = request.nextUrl.clone();
      url.pathname = next.path;
      return NextResponse.rewrite(url);
    }
    case "redirect":
      return NextResponse.redirect(new URL(`${next.path}${search}`, request.url));
  }
}

export const config = {
  // Pages only: not Next's own files, the API routes (the webhook), nor anything with an
  // extension (favicons, images).
  matcher: ["/((?!_next/|api/|.*\\..*).*)"],
};
