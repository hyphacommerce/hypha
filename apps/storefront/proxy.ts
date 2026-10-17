// Routing by country (design note 0023, SF3, SF4). With folders (the default), a path
// that doesn't start with a country the store sells to is redirected to the shopper's
// country folder. In a single tree (`STOREFRONT_ROUTING=single`), every page request is
// rewritten to that folder internally, so URLs carry no country. The country comes from
// the `country` cookie, then the host's geolocation header, then the store's first.

import { type NextRequest, NextResponse } from "next/server";

import { api } from "@/lib/api";
import { COUNTRY_COOKIE, pickCountry, routing } from "@/lib/routing";

/** The store's countries, kept for a minute: they change rarely, and every page asks. */
let cached: { codes: string[]; until: number } | undefined;

/**
 * The store's countries. Only an answer is kept: a failed call (the API restarting, a
 * timeout) would otherwise leave every path without a country unroutable for a minute.
 * It falls back to the last answer, if there was one.
 */
async function countryCodes(): Promise<string[]> {
  const now = Date.now();
  if (cached && cached.until > now) return cached.codes;
  try {
    const { data } = await api().GET("/storefront/countries");
    if (data) {
      cached = { codes: data.items.map((c) => c.country), until: now + 60_000 };
      return cached.codes;
    }
  } catch {
    // Unreachable: as a failed answer, below.
  }
  return cached?.codes ?? [];
}

export async function proxy(request: NextRequest) {
  const { pathname, search } = request.nextUrl;
  const codes = await countryCodes();
  const geolocated =
    request.headers.get("x-vercel-ip-country") ?? request.headers.get("cf-ipcountry");
  const country = pickCountry(codes, request.cookies.get(COUNTRY_COOKIE)?.value, geolocated);
  // No market sells anywhere yet: the pages say so (`[country]` answers 404).
  if (!country) return NextResponse.next();
  const folder = `/${country.toLowerCase()}`;

  if (routing(process.env.STOREFRONT_ROUTING) === "single") {
    const url = request.nextUrl.clone();
    url.pathname = `${folder}${pathname}`;
    return NextResponse.rewrite(url);
  }
  const first = pathname.split("/")[1]?.toUpperCase();
  if (first && codes.includes(first)) return NextResponse.next();
  const target = pathname === "/" ? folder : `${folder}${pathname}`;
  return NextResponse.redirect(new URL(`${target}${search}`, request.url));
}

export const config = {
  // Pages only: not Next's own files, the API routes (the webhook), nor anything with an
  // extension (favicons, images).
  matcher: ["/((?!_next/|api/|.*\\..*).*)"],
};
