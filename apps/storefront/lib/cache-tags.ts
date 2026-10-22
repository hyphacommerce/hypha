// The tags the storefront's cached reads carry, shared by the reads (`lib/api.ts`,
// `lib/listing-data.ts`) and the webhook that expires them
// (`app/api/revalidate/route.ts`). A tag on a read also lands on every static page that
// rendered it, so expiring the tag re-renders those pages on their next visit.

export const TAGS = {
  /** The store's details and countries. */
  store: "store",
  /** The category tree and every category page. */
  categories: "categories",
  category: (handle: string) => `category:${handle}`,
  /** Every product page. */
  products: "products",
  product: (handle: string) => `product:${handle}`,
  /** Search results: home's shelves, category listings, search. */
  listings: "listings",
  /** The custom field definitions pages name, order and label values by. */
  fields: "custom-fields",
  /** Everything priced: product pages and listings (a market or price list changed). */
  prices: "prices",
} as const;

/** How long a cached read stays fresh without a webhook, in seconds: the pages'. */
export const FRESH_SECONDS = 300;
