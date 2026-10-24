// What search engines and link previews read: plain-text descriptions, schema.org
// structured data (JSON-LD) and the per-country alternates. Pure: it reads nothing but
// its arguments, so pages, route handlers and tests share it.

type Money = { amount: number; currency: string };

/** What structured data reads of a storefront product. */
type Product = {
  title: string;
  description?: string | null;
  vendor?: string | null;
  product_type?: string | null;
  media: { kind: string; url: string }[];
  variants: { title?: string | null; price?: Money | null; available?: boolean | null }[];
};

/** Search engines cut descriptions near this length. */
const DESCRIPTION_LENGTH = 160;

const ENTITIES: Record<string, string> = {
  amp: "&",
  lt: "<",
  gt: ">",
  quot: '"',
  apos: "'",
  nbsp: " ",
};

/**
 * Rich text (the API's sanitised HTML) as one line of plain text, cut at a word
 * within `max` characters with an ellipsis; `undefined` for none.
 */
export function plainText(
  html: string | null | undefined,
  max = DESCRIPTION_LENGTH,
): string | undefined {
  if (!html) return undefined;
  const text = html
    .replace(/<[^>]*>/g, " ")
    .replace(/&(#\d+|#x[\da-f]+|\w+);/gi, (entity, name: string) => {
      if (name[0] === "#") {
        const code =
          name[1] === "x" || name[1] === "X" ? parseInt(name.slice(2), 16) : +name.slice(1);
        return Number.isFinite(code) ? String.fromCodePoint(code) : entity;
      }
      return ENTITIES[name.toLowerCase()] ?? entity;
    })
    .replace(/\s+/g, " ")
    .trim();
  if (!text) return undefined;
  if (text.length <= max) return text;
  const cut = text.slice(0, max - 1);
  const space = cut.lastIndexOf(" ");
  return `${(space > max / 2 ? cut.slice(0, space) : cut).replace(/[\s,.;:]+$/, "")}…`;
}

/** Minor units as schema.org's decimal price ("10.50"). */
export function decimal(money: Money, digits: number): string {
  const sign = money.amount < 0 ? "-" : "";
  const units = String(Math.abs(money.amount)).padStart(digits + 1, "0");
  return digits === 0
    ? `${sign}${units}`
    : `${sign}${units.slice(0, -digits)}.${units.slice(-digits)}`;
}

/**
 * Where each country's copy of a page lives, as `hreflang` alternates: the store's
 * language with each country (`en-HR`), and `x-default` for the first. Only with
 * country folders: a single tree has one URL per page.
 */
export function countryAlternates(
  language: string,
  countries: string[],
  pathFor: (country: string) => string,
): Record<string, string> | undefined {
  if (countries.length < 2) return undefined;
  const alternates: Record<string, string> = {};
  for (const c of countries) alternates[`${language}-${c}`] = pathFor(c);
  alternates["x-default"] = pathFor(countries[0]);
  return alternates;
}

/** A breadcrumb trail as a schema.org `BreadcrumbList`; `url`s absolute. */
export function breadcrumbLd(items: { name: string; url: string }[]) {
  return {
    "@context": "https://schema.org",
    "@type": "BreadcrumbList",
    itemListElement: items.map((item, i) => ({
      "@type": "ListItem",
      position: i + 1,
      name: item.name,
      item: item.url,
    })),
  };
}

/**
 * A product as schema.org's `Product`: its images, maker, and one `Offer` per variant
 * priced in the country (a `ProductGroup` would list variants by their options, which
 * Google reads only for apparel-like attributes; offers say what matters, the prices
 * and stock). `null` where it isn't sold, so no offer claims a price that isn't there.
 */
export function productLd(product: Product, url: string, digits: number) {
  const offers = product.variants.flatMap((v) =>
    v.price
      ? [
          {
            "@type": "Offer",
            ...(v.title ? { name: v.title } : {}),
            url,
            price: decimal(v.price, digits),
            priceCurrency: v.price.currency,
            availability:
              v.available === false
                ? "https://schema.org/OutOfStock"
                : "https://schema.org/InStock",
            itemCondition: "https://schema.org/NewCondition",
          },
        ]
      : [],
  );
  if (offers.length === 0) return null;
  const images = product.media.filter((m) => m.kind === "image").map((m) => m.url);
  const description = plainText(product.description, 5000);
  return {
    "@context": "https://schema.org",
    "@type": "Product",
    name: product.title,
    url,
    ...(description ? { description } : {}),
    ...(images.length > 0 ? { image: images } : {}),
    ...(product.vendor ? { brand: { "@type": "Brand", name: product.vendor } } : {}),
    ...(product.product_type ? { category: product.product_type } : {}),
    offers,
  };
}

/** The store as schema.org's `OnlineStore`, with its site: the home page's. */
export function storeLd(
  store: { name: string; description?: string | null; contact_email?: string | null },
  url: string,
) {
  return [
    {
      "@context": "https://schema.org",
      "@type": "OnlineStore",
      name: store.name,
      url,
      ...(store.description ? { description: store.description } : {}),
      ...(store.contact_email ? { email: store.contact_email } : {}),
    },
    {
      "@context": "https://schema.org",
      "@type": "WebSite",
      name: store.name,
      url,
    },
  ];
}
