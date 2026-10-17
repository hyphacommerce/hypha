import { CountryPicker } from "@/components/country-picker";
import { api, countryOf, loadCategories, loadCountries, loadStore } from "@/lib/api";
import { cartId, link } from "@/lib/site";

/** Local names where the runtime knows them, else the API's English one. */
function localName(code: string, english: string, language: string): string {
  try {
    return new Intl.DisplayNames([language, "en"], { type: "region" }).of(code) ?? english;
  } catch {
    return english;
  }
}

/** Every page of a country: header with navigation and the cart, footer (0023, §3). */
export default async function CountryLayout({
  children,
  params,
}: {
  children: React.ReactNode;
  params: Promise<{ country: string }>;
}) {
  const { country: folder } = await params;
  const country = await countryOf(folder);
  const code = country.country;
  const [store, countries, categories, cart] = await Promise.all([
    loadStore(),
    loadCountries(),
    loadCategories(),
    cartId(code),
  ]);
  let items = 0;
  if (cart) {
    const { data } = await api().GET("/storefront/carts/{id}", {
      params: { path: { id: cart } },
    });
    if (data && !data.order) items = data.lines.reduce((n, line) => n + line.quantity, 0);
  }

  return (
    <>
      <header className="header">
        <div className="page">
          <div className="header-row">
            <a className="brand" href={link(code, "/")}>
              {store.name}
            </a>
            <form className="search-box" action={link(code, "/search")} method="get">
              <input type="search" name="q" placeholder="Search" aria-label="Search" />
            </form>
            <CountryPicker
              current={code}
              countries={countries.map((c) => ({
                code: c.country,
                name: localName(c.country, c.name, store.default_language),
                currency: c.currency,
              }))}
            />
            <a href={link(code, "/cart")}>Cart{items > 0 ? ` (${items})` : ""}</a>
          </div>
          {categories.length > 0 && (
            <nav className="nav" aria-label="Categories">
              {categories.map((c) => (
                <a key={c.id} href={link(code, `/c/${c.handle}`)}>
                  {c.name}
                </a>
              ))}
            </nav>
          )}
        </div>
      </header>
      <main className="page">{children}</main>
      <footer className="footer">
        <div className="page">
          <p>{store.name}</p>
          {store.description && <p>{store.description}</p>}
          {store.contact_email && (
            <p>
              <a href={`mailto:${store.contact_email}`}>{store.contact_email}</a>
            </p>
          )}
        </div>
      </footer>
    </>
  );
}
