import type { Metadata } from "next";

import { api, countryOf, loaded } from "@/lib/api";
import { link } from "@/lib/site";

import { ProductForm } from "./product-form";

type Props = {
  params: Promise<{ country: string; handle: string }>;
  searchParams: Promise<{ error?: string }>;
};

async function load(handle: string, country: string) {
  return loaded(
    await api().GET("/storefront/products/by-handle/{handle}", {
      params: { path: { handle }, query: { country } },
    }),
  );
}

export async function generateMetadata({ params }: Props): Promise<Metadata> {
  const { country, handle } = await params;
  const product = await load(handle, country.toUpperCase());
  return { title: product.title };
}

/**
 * A product page in one read (design note 0021, SU7): priced and stocked in the
 * shopper's country. One the country's market doesn't sell is shown as such (0021, Q2);
 * a draft or unpublished one is a 404.
 */
export default async function ProductPage({ params, searchParams }: Props) {
  const { country: folder, handle } = await params;
  const country = await countryOf(folder);
  const code = country.country;
  const product = await load(handle, code);
  const { error } = await searchParams;
  const here = link(code, `/p/${handle}`);

  return (
    <>
      <nav className="breadcrumb" aria-label="Breadcrumb">
        <a href={link(code, "/")}>Home</a> / {product.title}
      </nav>
      {error && <p className="error">{error}</p>}
      {product.sold_in_country === false ? (
        <div className="stack" style={{ marginTop: 32 }}>
          <h1>{product.title}</h1>
          <p className="error">
            Not available in {country.name}. Pick another country above to see where it's
            sold.
          </p>
        </div>
      ) : (
        <ProductForm product={product} country={code} digits={country.minor_units} from={here} />
      )}
    </>
  );
}
