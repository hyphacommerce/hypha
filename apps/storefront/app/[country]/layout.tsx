import { Grid } from "@astryxdesign/core/Grid";
import { Layout, LayoutContent, LayoutFooter, LayoutHeader } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";

import { CartDrawer } from "@/components/cart/cart-drawer";
import { CartProvider } from "@/components/cart/cart-provider";
import { PageTransition } from "@/components/page-transition";
import { SiteHeader } from "@/components/site-header";
import { countryOf, loadCategories, loadCountries, loadStore } from "@/lib/api";
import { link } from "@/lib/site";

/** Pages under a country are static and regenerated in the background: none is built ahead,
 * since there's no API to read at build time; each is rendered on its first visit and kept,
 * then refreshed at most every five minutes. */
export const revalidate = 300;
export const dynamicParams = true;
export function generateStaticParams() {
  return [];
}

/** Local names where the runtime knows them, else the API's English one. */
function localName(code: string, english: string, language: string): string {
  try {
    return new Intl.DisplayNames([language, "en"], { type: "region" }).of(code) ?? english;
  } catch {
    return english;
  }
}

/** Every page of a country: the header, the cart drawer, the page, the footer. Nothing
 * here reads the request, so the pages under it can be static; the cart comes in the
 * browser (`CartProvider`). */
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
  const [store, countries, categories] = await Promise.all([
    loadStore(),
    loadCountries(),
    loadCategories(),
  ]);
  const base = link(code, "/").replace(/\/$/, "");
  const category = (c: { id: string; name: string; handle: string }) => ({
    id: c.id,
    name: c.name,
    href: link(code, `/c/${c.handle}`),
  });

  return (
    <CartProvider country={code} digits={country.minor_units} base={base}>
      <CartDrawer />
      <Layout
        height="auto"
        contentWidth={1280}
        padding={6}
        header={
          <LayoutHeader hasDivider>
            <SiteHeader
              storeName={store.name}
              country={code}
              homeHref={link(code, "/")}
              searchHref={link(code, "/search")}
              accountHref={link(code, "/account")}
              countries={countries.map((c) => ({
                code: c.country,
                name: localName(c.country, c.name, store.default_language),
                currency: c.currency,
              }))}
              categories={categories.map((c) => ({
                ...category(c),
                children: c.children.map(category),
              }))}
            />
          </LayoutHeader>
        }
        content={
          <LayoutContent>
            <PageTransition>
              <VStack gap={8}>{children}</VStack>
            </PageTransition>
          </LayoutContent>
        }
        footer={
          <LayoutFooter hasDivider>
            <VStack gap={6} paddingBlock={4}>
              <Grid columns={{ minWidth: 200, repeat: "fit" }} gap={6}>
                <VStack gap={2}>
                  <Text type="large" weight="semibold">
                    {store.name}
                  </Text>
                  {store.description && <Text color="secondary">{store.description}</Text>}
                </VStack>
                <VStack gap={2}>
                  <Text type="label">Shop</Text>
                  {categories.map((c) => (
                    <Link key={c.id} href={link(code, `/c/${c.handle}`)} color="secondary">
                      {c.name}
                    </Link>
                  ))}
                  <Link href={link(code, "/search")} color="secondary">
                    All products
                  </Link>
                </VStack>
                <VStack gap={2}>
                  <Text type="label">Help</Text>
                  <Link href={link(code, "/cart")} color="secondary">
                    Cart
                  </Link>
                  <Link href={link(code, "/account")} color="secondary">
                    Account
                  </Link>
                  {store.contact_email && (
                    <Link href={`mailto:${store.contact_email}`} color="secondary">
                      {store.contact_email}
                    </Link>
                  )}
                </VStack>
              </Grid>
              <HStack hAlign="between" wrap="wrap" gap={2}>
                <Text type="supporting" color="secondary">
                  © {store.name}
                </Text>
                <Text type="supporting" color="secondary">
                  Shipping to {country.name} · Prices in {country.currency}
                </Text>
              </HStack>
            </VStack>
          </LayoutFooter>
        }
      />
    </CartProvider>
  );
}
