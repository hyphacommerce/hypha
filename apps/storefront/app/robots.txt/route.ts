// What crawlers may visit: the shop's pages, not the shopper's own (cart, checkout,
// account, orders) nor internal search, whose endless word and filter combinations
// would crowd out the products. The sitemap lists the rest.

import { loadCountries } from "@/lib/api";
import { mode, siteUrl } from "@/lib/site";

const PRIVATE = ["/cart", "/checkout", "/account", "/search", "/order/"];

export async function GET(request: Request) {
  const origin = siteUrl()?.origin ?? new URL(request.url).origin;
  // Each country's folder spelled out: a `/*/search` wildcard would also match a
  // product whose handle starts with "search".
  const folders =
    mode() === "folders" ? (await loadCountries()).map((c) => `/${c.country.toLowerCase()}`) : [""];
  const lines = [
    "User-agent: *",
    ...folders.flatMap((f) => PRIVATE.map((p) => `Disallow: ${f}${p}`)),
    "Disallow: /order/",
    "Disallow: /api/",
    "",
    `Sitemap: ${origin}/sitemap.xml`,
    "",
  ];
  return new Response(lines.join("\n"), {
    headers: {
      "content-type": "text/plain; charset=utf-8",
      "cache-control": "public, max-age=3600",
    },
  });
}
