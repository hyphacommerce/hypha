import { Link } from "@astryxdesign/core/Link";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";

import { SiteHeader } from "@/components/site-header";
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

/** Every page of a country: header with search, the country and the cart; footer
 * (design note 0023, §3). */
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
      <SiteHeader
        storeName={store.name}
        country={code}
        homeHref={link(code, "/")}
        searchHref={link(code, "/search")}
        cartHref={link(code, "/cart")}
        cartCount={items}
        countries={countries.map((c) => ({
          code: c.country,
          name: localName(c.country, c.name, store.default_language),
          currency: c.currency,
        }))}
        categories={categories.map((c) => ({
          id: c.id,
          name: c.name,
          href: link(code, `/c/${c.handle}`),
        }))}
      />
      <main className="shop-page shop-main">{children}</main>
      <footer className="shop-footer">
        <div className="shop-page">
          <HStack gap={8} wrap="wrap" vAlign="start" hAlign="between">
            <VStack gap={2} maxWidth={420}>
              <Text type="large" weight="semibold">
                {store.name}
              </Text>
              {store.description && <Text color="secondary">{store.description}</Text>}
            </VStack>
            <VStack gap={2}>
              <Text type="label">Shop</Text>
              <Link href={link(code, "/search")} color="secondary">
                All products
              </Link>
              <Link href={link(code, "/cart")} color="secondary">
                Cart
              </Link>
              {store.contact_email && (
                <Link href={`mailto:${store.contact_email}`} color="secondary">
                  {store.contact_email}
                </Link>
              )}
            </VStack>
          </HStack>
        </div>
      </footer>
    </>
  );
}
