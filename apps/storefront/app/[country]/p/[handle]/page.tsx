import { Banner } from "@astryxdesign/core/Banner";
import { BreadcrumbItem, Breadcrumbs } from "@astryxdesign/core/Breadcrumbs";
import { Heading } from "@astryxdesign/core/Heading";
import { VStack } from "@astryxdesign/core/Stack";
import type { Metadata } from "next";
import { notFound } from "next/navigation";

import { AppElements } from "@/components/app-element";
import { countryOf, fieldContext, loadProduct } from "@/lib/api";
import { elementsFor } from "@/lib/app-elements";
import { presentFields } from "@/lib/custom-fields";
import { link } from "@/lib/site";

import { ProductForm } from "./product-form";

/** Static, refreshed at most every five minutes (design note 0024, §7). None is built
 * ahead (no API at build time): each product renders on its first visit, then is kept. */
export const revalidate = 300;
export function generateStaticParams() {
  return [];
}

type Props = {
  params: Promise<{ country: string; handle: string }>;
};

async function load(handle: string, country: string) {
  const product = await loadProduct(handle, country);
  if (!product) notFound();
  return product;
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
export default async function ProductPage({ params }: Props) {
  const { country: folder, handle } = await params;
  // Both at once: the folder is the country's code (the proxy lets only sold-to
  // countries' folders through), and an unknown one 404s either way.
  const [country, product, appBlocks] = await Promise.all([
    countryOf(folder),
    load(handle, folder.toUpperCase()),
    // What installed apps show on a product (design note 0029, §7): elements, loaded in
    // the browser, so the page stays static.
    elementsFor("product.block"),
  ]);
  const code = country.country;
  // The merchant's custom fields (design note 0033, §6.2), formatted here so the form
  // shows the selected variant's over the product's without formatting in the browser.
  const context = await fieldContext(
    country,
    product.custom_fields,
    ...product.variants.map((v) => v.custom_fields),
  );
  const fields = {
    product: presentFields("product", product.custom_fields, context),
    variants: Object.fromEntries(
      product.variants.map((v) => [v.id, presentFields("variant", v.custom_fields, context)]),
    ),
  };

  return (
    <VStack gap={5}>
      <Breadcrumbs variant="supporting">
        <BreadcrumbItem href={link(code, "/")}>Home</BreadcrumbItem>
        <BreadcrumbItem isCurrent>{product.title}</BreadcrumbItem>
      </Breadcrumbs>
      {product.sold_in_country === false ? (
        <VStack gap={4} maxWidth={640}>
          <Heading level={1} type="display-2">
            {product.title}
          </Heading>
          <Banner
            status="warning"
            title={`Not sold in ${country.name}`}
            description="Switch currency at the top to shop where it is sold."
          />
        </VStack>
      ) : (
        <ProductForm product={product} digits={country.minor_units} fields={fields} />
      )}
      <AppElements elements={appBlocks} props={{ productId: product.id }} />
    </VStack>
  );
}
