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
  /** Everything priced: product pages and listings (a market or price list changed, a
   * list's window opened or closed). */
  prices: "prices",
} as const;

/** What a translation or a language event changes: tags to expire, and the products
 * whose pages to expire by ID (the webhook finds their handles). */
export type LanguageChange = { tags: string[]; products: string[] };

/**
 * The cached reads a translation or language event affects; `undefined` for any other
 * event. Every language of a resource is under the same tags (a read's language is in its
 * key, not its tags), so a translation expires the resource in all of them.
 *
 * - `translation.updated` / `.deleted`, by `data.resource.kind`: a product's page; a
 *   category's name and description show in the header's tree and on every category
 *   page; a field definition's names on every page that shows its values; the store's
 *   description in every footer; anything else (search facets' labels) in listings.
 * - `store.updated`: the store's details (name, description) and, for `languages` or
 *   `default_language`, every country's languages: all under `store`.
 */
export function languageChange(
  type: string,
  data: Record<string, unknown>,
): LanguageChange | undefined {
  switch (type) {
    case "translation.updated":
    case "translation.deleted": {
      const resource = (data.resource ?? {}) as { kind?: unknown; id?: unknown };
      switch (resource.kind) {
        case "product":
          return typeof resource.id === "string"
            ? { tags: [], products: [resource.id] }
            : { tags: [TAGS.products], products: [] };
        case "category":
          return { tags: [TAGS.categories], products: [] };
        case "custom_field_definition":
          return { tags: [TAGS.fields], products: [] };
        case "store":
          return { tags: [TAGS.store], products: [] };
        default:
          return { tags: [TAGS.listings], products: [] };
      }
    }
    case "store.updated":
      return { tags: [TAGS.store], products: [] };
    default:
      return undefined;
  }
}

/** How long a cached read stays fresh without a webhook, in seconds: the pages'. */
export const FRESH_SECONDS = 300;
