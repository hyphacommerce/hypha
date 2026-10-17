import type { StorefrontSchemas } from "@hypha/sdk/storefront";

import { formatRange } from "@/lib/money";

type Hit = StorefrontSchemas["SearchHitResponse"];

/** Search results as product cards: image, title, price (in the country's digits). */
export function ProductGrid({
  hits,
  digits,
  hrefOf,
}: {
  hits: Hit[];
  digits: number;
  hrefOf: (hit: Hit) => string;
}) {
  return (
    <div className="grid">
      {hits.map((hit) => {
        const image = hit.image?.kind === "image" ? (hit.image.thumbnail_url ?? hit.image.url) : null;
        return (
          <a key={hit.listing} className="card" href={hrefOf(hit)}>
            <div className="card-image">
              {image ? (
                // Plain <img>: the media URLs resize themselves (design note 0014).
                <img src={image} alt={hit.image?.alt ?? ""} loading="lazy" />
              ) : (
                <span className="muted">No image</span>
              )}
            </div>
            <span className="card-title">{hit.title}</span>
            <span className="muted">
              {formatRange(hit.listing_price.min, hit.listing_price.max, digits)}
              {!hit.available && " · Sold out"}
            </span>
          </a>
        );
      })}
    </div>
  );
}
