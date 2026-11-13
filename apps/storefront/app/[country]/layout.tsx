import { Grid } from "@astryxdesign/core/Grid";
import { Layout, LayoutContent, LayoutFooter, LayoutHeader } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";

import { CartDrawer } from "@/components/cart/cart-drawer";
import { CartProvider } from "@/components/cart/cart-provider";
import { ClientMessages } from "@/components/messages";
import { PageTransition } from "@/components/page-transition";
import { SiteHeader } from "@/components/site-header";
import { countryOf, loadCategories, loadCountries, loadStore } from "@/lib/api";
import { countryName } from "@/lib/format";
import { pageT } from "@/lib/i18n";
import { SHARED_NAMESPACES } from "@/lib/messages";
import { link } from "@/lib/site";

/** Pages under a country are static and regenerated in the background: none is built ahead,
 * since there's no API to read at build time; each is rendered on its first visit and kept,
 * then refreshed at most every five minutes. */
export const revalidate = 300;
export const dynamicParams = true;
export function generateStaticParams() {
  return [];
}

/** Every page of a country: the header, the cart drawer, the page, the footer, and the
 * page's language for the client components under it (`ClientMessages`: the namespaces
 * every page reads). Nothing here reads the request, so the pages under it can be
 * static; the cart comes in the browser (`CartProvider`). */
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
  const [store, countries, categories, t] = await Promise.all([
    loadStore(),
    loadCountries(),
    loadCategories(),
    pageT(folder),
  ]);
  const base = link(code, "/").replace(/\/$/, "");
  const category = (c: { id: string; name: string; handle: string }) => ({
    id: c.id,
    name: c.name,
    href: link(code, `/c/${c.handle}`),
  });

  return (
    <ClientMessages t={t} namespaces={SHARED_NAMESPACES}>
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
                  name: countryName(c.country, c.name, t.locale),
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
                    <Text type="label">{t("footer.shop")}</Text>
                    {categories.map((c) => (
                      <Link key={c.id} href={link(code, `/c/${c.handle}`)} color="secondary">
                        {c.name}
                      </Link>
                    ))}
                    <Link href={link(code, "/search")} color="secondary">
                      {t("footer.all_products")}
                    </Link>
                    <Link href={link(code, "/sale")} color="secondary">
                      {t("footer.sale")}
                    </Link>
                  </VStack>
                  <VStack gap={2}>
                    <Text type="label">{t("footer.help")}</Text>
                    <Link href={link(code, "/cart")} color="secondary">
                      {t("footer.cart")}
                    </Link>
                    <Link href={link(code, "/account")} color="secondary">
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
    </ClientMessages>
  );
}
