// A product listing through search (design note 0023, SF7): a category page and the
// search page are both this, with `category` or `q`. Facets filter, sorting and paging
// keep to the URL, so every state can be linked to.

import { api } from "@/lib/api";
import type { Country } from "@/lib/api";

import { ProductGrid } from "./product-grid";

export type ListingParams = Record<string, string | string[] | undefined>;

const SORTS = [
  { value: "relevance", label: "Best match" },
  { value: "newest", label: "Newest" },
  { value: "price_asc", label: "Price: low to high" },
  { value: "price_desc", label: "Price: high to low" },
  { value: "title", label: "Name" },
] as const;

type Sort = (typeof SORTS)[number]["value"];

function list(value: string | string[] | undefined): string[] {
  return value === undefined ? [] : Array.isArray(value) ? value : [value];
}

function one(value: string | string[] | undefined): string | undefined {
  return Array.isArray(value) ? value[0] : value;
}

export async function Listing({
  country,
  params,
  base,
  category,
  productHref,
}: {
  country: Country;
  params: ListingParams;
  /** This page's URL, without a query. */
  base: string;
  category?: string;
  productHref: (handle: string) => string;
}) {
  const q = one(params.q)?.trim() || undefined;
  const filters = list(params.filter);
  const sortParam = one(params.sort);
  const sort: Sort = SORTS.some((s) => s.value === sortParam)
    ? (sortParam as Sort)
    : q
      ? "relevance"
      : "newest";
  const cursor = one(params.cursor);

  const { data, error } = await api().GET("/storefront/search", {
    params: {
      query: {
        country: country.country,
        q,
        filter: filters,
        category,
        sort,
        cursor,
        limit: 24,
      },
    },
  });
  if (error) {
    // A stale cursor or filter in a hand-edited URL: start the listing again.
    return (
      <p className="error">
        This listing couldn't be shown ({error.error}). <a href={base}>Start again</a>
      </p>
    );
  }

  /** This listing's URL with `change` applied (and the page reset). */
  const url = (change: { filter?: string[]; sort?: string; cursor?: string }) => {
    const query = new URLSearchParams();
    if (q) query.set("q", q);
    for (const f of change.filter ?? filters) query.append("filter", f);
    const s = change.sort ?? sortParam;
    if (s) query.set("sort", s);
    if (change.cursor) query.set("cursor", change.cursor);
    const text = query.toString();
    return text ? `${base}?${text}` : base;
  };
  const toggle = (value: string) =>
    url({
      filter: filters.includes(value) ? filters.filter((f) => f !== value) : [...filters, value],
    });

  const facets = data.facets.filter((f) => f.values.length > 0);
  return (
    <div className="listing">
      <aside>
        {facets.map((facet) => (
          <div key={facet.key} className="facet">
            <h3>{facet.label ?? facetName(facet.key)}</h3>
            <ul>
              {facet.values.slice(0, 12).map((v) => {
                const value = `${facet.key}:${v.value}`;
                const on = filters.includes(value);
                return (
                  <li key={v.value}>
                    <a href={toggle(value)} aria-current={on}>
                      {on ? "✓ " : ""}
                      {v.value} <span className="muted">({v.count})</span>
                    </a>
                  </li>
                );
              })}
            </ul>
          </div>
        ))}
      </aside>
      <section>
        <div className="toolbar">
          <span className="muted">
            {data.total} {data.total === 1 ? "product" : "products"}
          </span>
          <form method="get" action={base}>
            {q && <input type="hidden" name="q" value={q} />}
            {filters.map((f) => (
              <input key={f} type="hidden" name="filter" value={f} />
            ))}
            <label>
              <span className="muted" style={{ marginRight: 6 }}>
                Sort
              </span>
              <select name="sort" defaultValue={sort}>
                {SORTS.map((s) => (
                  <option key={s.value} value={s.value}>
                    {s.label}
                  </option>
                ))}
              </select>
            </label>{" "}
            <button className="button button-quiet" type="submit">
              Apply
            </button>
          </form>
        </div>
        {data.items.length === 0 ? (
          <p className="muted">Nothing here yet.</p>
        ) : (
          <ProductGrid
            hits={data.items}
            digits={country.minor_units}
            hrefOf={(hit) => productHref(hit.handle)}
          />
        )}
        <div className="pager">
          {cursor && (
            <a className="button button-quiet" href={url({})}>
              First page
            </a>
          )}
          {data.next_cursor && (
            <a className="button button-quiet" href={url({ cursor: data.next_cursor })}>
              Next page
            </a>
          )}
        </div>
      </section>
    </div>
  );
}

/** `option.color` → "Color", `product_type` → "Type". */
function facetName(key: string): string {
  if (key.startsWith("option.")) {
    const name = key.slice("option.".length);
    return name.charAt(0).toUpperCase() + name.slice(1);
  }
  return { vendor: "Brand", product_type: "Type", tag: "Tag" }[key] ?? key;
}
