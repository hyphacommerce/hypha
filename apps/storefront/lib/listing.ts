// A listing's state in its URL: the words, the facet filters, the price
// range, the sort and the page, so every state can be linked to and the back button
// works. Shared by the server page and the client controls.

export type ListingParams = Record<string, string | string[] | undefined>;

export const SORTS = [
  { value: "relevance", label: "Best match" },
  { value: "newest", label: "Newest" },
  { value: "price_asc", label: "Price: low to high" },
  { value: "price_desc", label: "Price: high to low" },
  { value: "title", label: "Name" },
] as const;

export type Sort = (typeof SORTS)[number]["value"];

/** The first page's place in `back`: the stack can't hold an empty cursor. */
const FIRST = "-";

/** How many pages back Previous can go; older ones fall off (the URL stays short). */
const MAX_BACK = 30;

export type ListingState = {
  q?: string;
  /** `key:value` facet filters, as the API takes them (`option.color:red`,
   * `field.custom.material:wool`: the key ends at its first colon). */
  filters: string[];
  sort?: Sort;
  /** The price range in major units, as the shopper typed it ("10", "24.5"). */
  min?: string;
  max?: string;
  /** This page's cursor; none for the first page. */
  cursor?: string;
  /** The cursors of the pages before this one, oldest first, so Previous works
   * although the API pages forward only. */
  back: string[];
};

function all(value: string | string[] | undefined): string[] {
  return value === undefined ? [] : Array.isArray(value) ? value : [value];
}

function one(value: string | string[] | undefined): string | undefined {
  const v = Array.isArray(value) ? value[0] : value;
  return v?.trim() || undefined;
}

export function isSort(value: string | undefined): value is Sort {
  return SORTS.some((s) => s.value === value);
}

/** A price as typed, if it is one: a non-negative number. */
function price(value: string | undefined): string | undefined {
  return value !== undefined && /^\d+(\.\d+)?$/.test(value) ? value : undefined;
}

export function readListing(params: ListingParams): ListingState {
  const sort = one(params.sort);
  return {
    q: one(params.q),
    filters: all(params.filter),
    sort: isSort(sort) ? sort : undefined,
    min: price(one(params.min)),
    max: price(one(params.max)),
    cursor: one(params.cursor),
    back: all(params.back).slice(-MAX_BACK),
  };
}

/** The sort in effect: the shopper's, else best match for words and newest without. */
export function sortOf(state: ListingState): Sort {
  return state.sort ?? (state.q ? "relevance" : "newest");
}

/** `state` as a URL under `base`. */
export function listingHref(base: string, state: ListingState): string {
  const query = new URLSearchParams();
  if (state.q) query.set("q", state.q);
  for (const f of state.filters) query.append("filter", f);
  if (state.min) query.set("min", state.min);
  if (state.max) query.set("max", state.max);
  if (state.sort) query.set("sort", state.sort);
  if (state.cursor) {
    query.set("cursor", state.cursor);
    for (const b of state.back) query.append("back", b);
  }
  const text = query.toString();
  return text ? `${base}?${text}` : base;
}

/** The same listing narrowed or sorted differently, from its first page. */
export function changed(state: ListingState, change: Partial<ListingState>): ListingState {
  return { ...state, ...change, cursor: undefined, back: [] };
}

/** The page after this one. */
export function nextPage(state: ListingState, cursor: string): ListingState {
  return { ...state, cursor, back: [...state.back, state.cursor ?? FIRST].slice(-MAX_BACK) };
}

/** The page before this one, or `undefined` on the first page. */
export function previousPage(state: ListingState): ListingState | undefined {
  if (!state.cursor) return undefined;
  const back = [...state.back];
  const previous = back.pop();
  return { ...state, cursor: previous && previous !== FIRST ? previous : undefined, back };
}

/** Major units as typed → the API's minor units ("10.5" with 2 digits → 1050). */
export function toMinor(value: string | undefined, digits: number): number | undefined {
  if (value === undefined) return undefined;
  const [whole, fraction = ""] = value.split(".");
  const cents = (fraction + "0".repeat(digits)).slice(0, digits);
  const minor = Number(whole) * 10 ** digits + (digits > 0 ? Number(cents) : 0);
  return Number.isSafeInteger(minor) ? minor : undefined;
}

/** The filter that keeps only what's shown with a compare-at price. */
export const ON_SALE = "on_sale:true";

/** Whether the listing shows only what's on sale. */
export function onSaleOnly(state: ListingState): boolean {
  return state.filters.includes(ON_SALE);
}

/** The listing with or without the on-sale filter, from its first page. Any other
 * `on_sale` value goes: the toggle is the one way to set it. */
export function withOnSale(state: ListingState, on: boolean): ListingState {
  const filters = state.filters.filter((f) => !f.startsWith("on_sale:"));
  return changed(state, { filters: on ? [...filters, ON_SALE] : filters });
}

/** `option.color` → "Color", `field.custom.skin_type` → "Skin type" (a custom field: its
 * namespace isn't shown), `product_type` → "Type". */
export function facetName(key: string): string {
  if (key.startsWith("option.")) {
    const name = key.slice("option.".length);
    return name.charAt(0).toUpperCase() + name.slice(1);
  }
  if (key.startsWith("field.")) {
    const field = key.slice("field.".length);
    const name = field.slice(field.indexOf(".") + 1).replaceAll("_", " ");
    return name.charAt(0).toUpperCase() + name.slice(1);
  }
  return { vendor: "Brand", product_type: "Type", tag: "Tag", on_sale: "On sale" }[key] ?? key;
}
