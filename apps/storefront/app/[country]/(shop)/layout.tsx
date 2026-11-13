import { Grid } from "@astryxdesign/core/Grid";
import { Layout, LayoutContent, LayoutFooter, LayoutHeader } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";

import { CartDrawer } from "@/components/cart/cart-drawer";
import { CartProvider } from "@/components/cart/cart-provider";
import { PageTransition } from "@/components/page-transition";
import { SiteHeader } from "@/components/site-header";
import { countryOf, loadCategories, loadCountries, loadStore, pageLocale } from "@/lib/api";
import { countryName, languageName } from "@/lib/format";
import { pageT } from "@/lib/i18n";
import { localeSegment } from "@/lib/routing";
import { link } from "@/lib/site";

/**
 * Every page of a place: the header, the cart drawer, the page and the footer. A place
 * nobody sells to is a 404 here, below the root layout, so its not-found page
 * (`app/[country]/not-found.tsx`) still has the document and the words around it.
 * Nothing here reads the request, so the pages under it can be static; the cart comes in
 * the browser (`CartProvider`).
 */
export default async function ShopLayout({
  children,
  params,
}: {
  children: React.ReactNode;
  params: Promise<{ country: string }>;
}) {
  const { country: segment } = await params;
  const [country, place, countries, t] = await Promise.all([
    countryOf(segment),
    pageLocale(segment),
    loadCountries(),
    pageT(segment),
  ]);
  const code = country.country;
  const [store, categories] = await Promise.all([
    loadStore(code, place.language),
    loadCategories(code, place.language),
  ]);
  const here = place.segment;
  const home = link(here, "/");
  const category = (c: { id: string; name: string; handle: string }) => ({
    id: c.id,
    name: c.name,
    href: link(here, `/c/${c.handle}`),
  });

  return (
    <CartProvider country={code} digits={country.minor_units} base={home.replace(/\/$/, "")}>
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
              homeHref={home}
              searchHref={link(here, "/search")}
              accountHref={link(here, "/account")}
              countries={countries.map((c) => ({
                code: c.country,
                name: countryName(c.country, c.name, t.locale),
                currency: c.currency,
              }))}
              languages={country.languages.map((l) => ({
                segment: localeSegment(country, l),
                name: languageName(l),
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
                  <Text type="label">{t("footer.shop")}</Text>
                  {categories.map((c) => (
                    <Link key={c.id} href={link(here, `/c/${c.handle}`)} color="secondary">
                      {c.name}
                    </Link>
                  ))}
                  <Link href={link(here, "/search")} color="secondary">
                    {t("footer.all_products")}
                  </Link>
                  <Link href={link(here, "/sale")} color="secondary">
                    {t("footer.sale")}
                  </Link>
                </VStack>
                <VStack gap={2}>
                  <Text type="label">{t("footer.help")}</Text>
                  <Link href={link(here, "/cart")} color="secondary">
                    {t("footer.cart")}
                  </Link>
                  <Link href={link(here, "/account")} color="secondary">
                    {t("footer.account")}
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
                  {/* Hypha's credit, like a theme's: a merchant may remove it. */}
                  {t.rich("footer.credit", {
                    store: store.name,
                    link: (chunks, key) => (
                      <a key={key} className="shop-built-with" href="https://hyphacommerce.com">
                        {chunks}
                      </a>
                    ),
                  })}
                </Text>
                <Text type="supporting" color="secondary">
                  {t("footer.shipping_to", {
                    country: countryName(code, country.name, t.locale),
                    currency: country.currency,
                  })}
                </Text>
              </HStack>
            </VStack>
          </LayoutFooter>
        }
      />
    </CartProvider>
  );
}
