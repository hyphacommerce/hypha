// How URLs carry the shopper's country. Shared by the proxy, server pages and
// client components: it reads nothing but its arguments.

/** The cookie holding the shopper's country code (`HR`). */
export const COUNTRY_COOKIE = "country";

export type Routing = "folders" | "single";

/** `STOREFRONT_ROUTING`: `single` keeps the country out of URLs; anything else is folders. */
export function routing(value: string | undefined): Routing {
  return value === "single" ? "single" : "folders";
}

/**
 * A link to `path` (`/p/mug`, `/`) for `country` (`HR`): `/hr/p/mug` with folders, the
 * path itself in a single tree, where the proxy adds the country back.
 */
export function href(mode: Routing, country: string, path: string): string {
  if (mode === "single") return path;
  const folder = `/${country.toLowerCase()}`;
  // No trailing slash: Next.js redirects `/hr/` to `/hr` anyway.
  return path === "/" ? folder : `${folder}${path}`;
}

/** The country to use, from the cookie, then the host's geolocation, then the first. */
export function pickCountry(
  available: string[],
  cookie: string | undefined,
  geolocated: string | undefined | null,
): string | undefined {
  for (const candidate of [cookie, geolocated]) {
    const code = candidate?.toUpperCase();
    if (code && available.includes(code)) return code;
  }
  return available[0];
}
