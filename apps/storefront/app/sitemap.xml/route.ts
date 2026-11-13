// Every page worth finding, in every place it has a URL (each country in each of its
// languages: `alternatePlaces`): home, the sale, each shown category and each product the
// country's market sells, with the page's other copies as alternates.

import { loadCategories, loadCountries } from "@/lib/api";
import { search } from "@/lib/listing-data";
import { alternatePlaces, hreflangPlaces } from "@/lib/routing";
import { localeAlternates } from "@/lib/seo";
import { link, mode, segmentCountry, siteUrl } from "@/lib/site";

/** Search answers 100 a page and goes 1,000 results deep. */
const PAGE = 100;
const MAX_PAGES = 10;

type Node = { handle: string; children: Node[] };

function handles(nodes: Node[]): string[] {
  return nodes.flatMap((n) => [n.handle, ...handles(n.children)]);
}

/** The handles of what `country`'s market sells, newest first. */
// shortcut: the first 1,000 per country (search's depth); a larger catalogue needs a
// sitemap index paged by the API instead.
async function productHandles(country: string): Promise<string[]> {
  const found: string[] = [];
  let cursor: string | undefined;
  for (let page = 0; page < MAX_PAGES; page++) {
    const { data } = await search({
      country,
      group: "product",
      sort: "newest",
      limit: PAGE,
      cursor,
    });
    if (!data) break;
    found.push(...data.items.map((h) => h.handle));
    if (!data.next_cursor) break;
    cursor = data.next_cursor;
  }
  return found;
}

const escape = (s: string) =>
  s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");

type Place = { tag: string; segment: string };

export async function GET(request: Request) {
  const origin = siteUrl()?.origin ?? new URL(request.url).origin;
  const [countries, categories] = await Promise.all([loadCountries(), loadCategories()]);
  const places = alternatePlaces(mode(), countries);
  // What alternates may name (none where a single tree's URLs have no one language).
  const named = new Set(hreflangPlaces(mode(), countries).map((p) => p.segment));
  const codes = [...new Set(places.map((p) => segmentCountry(p.segment)))];
  const perCountry = new Map(
    await Promise.all(codes.map(async (c) => [c, await productHandles(c)] as const)),
  );
  // Where each product is sold, for its alternates: only those places have its page.
  const sellers = new Map<string, Set<string>>();
  for (const [country, products] of perCountry)
    for (const h of products) sellers.set(h, (sellers.get(h) ?? new Set()).add(country));

  const entry = (place: Place, path: string, alsoIn: Place[]) => {
    const alternates = localeAlternates(
      alsoIn
        .filter((p) => named.has(p.segment))
        .map((p) => ({ tag: p.tag, href: link(p.segment, path) })),
    );
    return [
      "<url>",
      `<loc>${escape(origin + link(place.segment, path))}</loc>`,
      ...Object.entries(alternates ?? {}).map(
        ([lang, href]) =>
          `<xhtml:link rel="alternate" hreflang="${lang}" href="${escape(origin + href)}"/>`,
      ),
      "</url>",
    ].join("");
  };
  const urls = places.flatMap((place) => {
    const products = perCountry.get(segmentCountry(place.segment)) ?? [];
    return [
      entry(place, "/", places),
      entry(place, "/sale", places),
      ...handles(categories).map((h) => entry(place, `/c/${h}`, places)),
      ...products.map((h) =>
        entry(
          place,
          `/p/${h}`,
          places.filter((p) => sellers.get(h)?.has(segmentCountry(p.segment))),
        ),
      ),
    ];
  });
  const xml = [
    '<?xml version="1.0" encoding="UTF-8"?>',
    '<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9" xmlns:xhtml="http://www.w3.org/1999/xhtml">',
    ...urls,
    "</urlset>",
    "",
  ].join("\n");
  return new Response(xml, {
    headers: {
      "content-type": "application/xml; charset=utf-8",
      "cache-control": "public, max-age=3600",
    },
  });
}
