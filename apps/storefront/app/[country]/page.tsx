import { ProductGrid } from "@/components/product-grid";
import { api, countryOf, loadCategories, loadStore } from "@/lib/api";
import { link } from "@/lib/site";

/** Home (design note 0023, §3): the store, its top categories, its newest products. */
export default async function HomePage({ params }: { params: Promise<{ country: string }> }) {
  const country = await countryOf((await params).country);
  const code = country.country;
  const [store, categories, newest] = await Promise.all([
    loadStore(),
    loadCategories(),
    api().GET("/storefront/search", {
      params: { query: { country: code, sort: "newest", limit: 8 } },
    }),
  ]);
  const products = newest.data?.items ?? [];

  return (
    <>
      <section style={{ padding: "56px 0 24px" }}>
        <h1 style={{ fontSize: "2.4rem", margin: 0 }}>{store.name}</h1>
        {store.description && (
          <p className="muted" style={{ maxWidth: 560, fontSize: "1.1rem" }}>
            {store.description}
          </p>
        )}
      </section>

      {categories.length > 0 && (
        <section>
          <h2>Shop by category</h2>
          <div className="chips">
            {categories.map((c) => (
              <a key={c.id} className="chip" href={link(code, `/c/${c.handle}`)}>
                {c.name}
              </a>
            ))}
          </div>
        </section>
      )}

      <section>
        <h2>New in</h2>
        {products.length === 0 ? (
          <p className="muted">No products for sale here yet.</p>
        ) : (
          <ProductGrid
            hits={products}
            digits={country.minor_units}
            hrefOf={(hit) => link(code, `/p/${hit.handle}`)}
          />
        )}
      </section>
    </>
  );
}
