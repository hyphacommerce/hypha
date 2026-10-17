"use client";

// Choosing a variant by its options, and adding it to the cart (design note 0023, §3).
// The price, availability and image follow the choice; nothing is fetched here: the
// page passed every variant's price in the shopper's country.

import type { StorefrontSchemas } from "@hypha/sdk/storefront";
import { useState } from "react";

import { addToCart } from "@/app/[country]/actions";
import { formatMoney } from "@/lib/money";

type Product = StorefrontSchemas["StorefrontProductResponse"];
type Variant = StorefrontSchemas["StorefrontVariantResponse"];

/** The variant with exactly these selections. */
function variantWith(product: Product, selections: string[]): Variant | undefined {
  return product.variants.find((v) => v.selections.every((s, i) => s === selections[i]));
}

export function ProductForm({
  product,
  country,
  digits,
  from,
}: {
  product: Product;
  country: string;
  digits: number;
  /** This page's URL, to come back to with an error. */
  from: string;
}) {
  // Start on the first variant that can be bought, else the first.
  const start = product.variants.find((v) => v.available) ?? product.variants[0];
  const [selections, setSelections] = useState<string[]>(start?.selections ?? []);
  const variant = variantWith(product, selections);
  const media = product.media.find((m) => m.asset_id === variant?.media_asset_id) ?? product.media[0];

  const pick = (option: number, value: string) => {
    const next = [...selections];
    next[option] = value;
    setSelections(next);
  };

  return (
    <div className="product">
      <div className="gallery">
        <div className="gallery-main">
          {media?.kind === "image" ? (
            <img src={media.url} alt={media.alt ?? product.title} />
          ) : null}
        </div>
        {product.media.length > 1 && (
          <div className="thumbs">
            {product.media
              .filter((m) => m.kind === "image")
              .map((m) => (
                <img key={m.asset_id} src={m.thumbnail_url ?? m.url} alt={m.alt ?? ""} />
              ))}
          </div>
        )}
      </div>

      <div className="stack">
        <div>
          {product.vendor && <p className="muted" style={{ margin: 0 }}>{product.vendor}</p>}
          <h1 style={{ margin: "4px 0 8px" }}>{product.title}</h1>
          {variant?.price ? (
            <div className="price">
              {formatMoney(variant.price, digits)}
              {product.prices_include_tax === false && (
                <span className="muted" style={{ fontSize: "0.85rem", marginLeft: 8 }}>
                  plus tax
                </span>
              )}
            </div>
          ) : null}
        </div>

        {product.options.length > 0 && (
          <div className="options">
            {product.options.map((option, index) => (
              <fieldset key={option.id}>
                <legend>{option.name}</legend>
                <div className="option-values">
                  {option.values.map((value) => {
                    const would = [...selections];
                    would[index] = value.id;
                    const candidate = variantWith(product, would);
                    return (
                      <button
                        key={value.id}
                        type="button"
                        className="option-value"
                        aria-pressed={selections[index] === value.id}
                        data-unavailable={!candidate?.available}
                        onClick={() => pick(index, value.id)}
                      >
                        {value.label}
                      </button>
                    );
                  })}
                </div>
              </fieldset>
            ))}
          </div>
        )}

        <form action={addToCart} className="stack">
          <input type="hidden" name="country" value={country} />
          <input type="hidden" name="variant" value={variant?.id ?? ""} />
          <input type="hidden" name="from" value={from} />
          <label style={{ maxWidth: 120 }}>
            Quantity
            <input type="number" name="quantity" min={1} defaultValue={1} />
          </label>
          <button className="button" type="submit" disabled={!variant?.available}>
            {!variant
              ? "This combination isn't sold"
              : variant.available
                ? "Add to cart"
                : "Sold out"}
          </button>
        </form>

        {product.description && (
          <div
            className="description"
            // Sanitised by the API (scripts, styles and event attributes removed).
            dangerouslySetInnerHTML={{ __html: product.description }}
          />
        )}
      </div>
    </div>
  );
}
