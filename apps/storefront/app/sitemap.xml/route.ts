// Every page worth finding, per country: home, the sale, each shown category and each
// product the country's market sells, with the other countries' copies as alternates.

import { loadCategories, loadCountries, loadStore } from "@/lib/api";
import { search } from "@/lib/listing-data";
import { countryAlternates } from "@/lib/seo";
import { link, mode, siteUrl } from "@/lib/site";

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

export async function GET(request: Request) {
  const origin = siteUrl()?.origin ?? new URL(request.url).origin;
  const [store, countries, categories] = await Promise.all([
    loadStore(),
    loadCountries(),
    loadCategories(),
  ]);
  // A single tree has one URL per page, its country from a cookie: the first's.
  const codes = countries.map((c) => c.country);
  const listed = mode() === "folders" ? codes : codes.slice(0, 1);
  const perCountry = await Promise.all(
    listed.map(async (country) => ({ country, products: await productHandles(country) })),
  );
  const sellers = new Map<string, string[]>();
  for (const { country, products } of perCountry)
    for (const h of products) sellers.set(h, [...(sellers.get(h) ?? []), country]);

  const entry = (country: string, path: string, alsoIn: string[]) => {
    const alternates =
      mode() === "folders"
        ? countryAlternates(store.default_language, alsoIn, (c) => link(c, path))
        : undefined;
    return [
      "<url>",
      `<loc>${escape(origin + link(country, path))}</loc>`,
      ...Object.entries(alternates ?? {}).map(
        ([lang, href]) =>
          `<xhtml:link rel="alternate" hreflang="${lang}" href="${escape(origin + href)}"/>`,
      ),
      "</url>",
    ].join("");
  };
  const urls = perCountry.flatMap(({ country, products }) => [
    entry(country, "/", listed),
    entry(country, "/sale", listed),
    ...handles(categories).map((h) => entry(country, `/c/${h}`, listed)),
    ...products.map((h) => entry(country, `/p/${h}`, sellers.get(h) ?? [country])),
  ]);
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
