import { Banner } from "@astryxdesign/core/Banner";
import { BreadcrumbItem, Breadcrumbs } from "@astryxdesign/core/Breadcrumbs";
import { Heading } from "@astryxdesign/core/Heading";
import { VStack } from "@astryxdesign/core/Stack";
import type { Metadata } from "next";
import { notFound } from "next/navigation";

import { AppElements } from "@/components/app-element";
import { JsonLd } from "@/components/json-ld";
import { ProductGrid } from "@/components/product-grid";
import { countryOf, fieldContext, loadProduct } from "@/lib/api";
import { elementsFor } from "@/lib/app-elements";
import { presentFields } from "@/lib/custom-fields";
import { relatedProducts } from "@/lib/listing-data";
import { pageMetadata } from "@/lib/metadata";
import { breadcrumbLd, plainText, productLd } from "@/lib/seo";
import { absolute, link } from "@/lib/site";

import { ProductForm } from "./product-form";

/** Static, refreshed at most every five minutes. None is built ahead (no API at build
 * time): each product renders on its first visit, then is kept. */
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
  const code = country.toUpperCase();
  const product = await load(handle, code);
  const image = product.media.find((m) => m.kind === "image");
  return pageMetadata({
    country: code,
    path: `/p/${handle}`,
    title: product.title,
    description: plainText(product.description),
    image: image && { ...image, alt: image.alt ?? product.title },
  });
}

/**
 * A product page in one read: priced and stocked in the shopper's country. One the
 * country's market doesn't sell is shown as such; a draft or unpublished one is a 404.
 */
export default async function ProductPage({ params }: Props) {
  const { country: folder, handle } = await params;
  // Both at once: the folder is the country's code (the proxy lets only sold-to
  // countries' folders through), and an unknown one 404s either way.
  const [country, product, appBlocks] = await Promise.all([
    countryOf(folder),
    load(handle, folder.toUpperCase()),
    // What installed apps show on a product: elements, loaded in the browser, so the
    // page stays static.
    elementsFor("product.block"),
  ]);
  const code = country.country;
  // The merchant's custom fields, formatted here so the form shows the selected
  // variant's over the product's without formatting in the browser.
  const [context, related] = await Promise.all([
    fieldContext(country, product.custom_fields, ...product.variants.map((v) => v.custom_fields)),
    relatedProducts(product, code),
  ]);
  const url = absolute(link(code, `/p/${product.handle}`));
  const fields = {
    product: presentFields("product", product.custom_fields, context),
    variants: Object.fromEntries(
      product.variants.map((v) => [v.id, presentFields("variant", v.custom_fields, context)]),
    ),
  };

  return (
    <VStack gap={5}>
      <JsonLd data={productLd(product, url, country.minor_units)} />
      <JsonLd
        data={breadcrumbLd([
          { name: "Home", url: absolute(link(code, "/")) },
          { name: product.title, url },
        ])}
      />
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
      {related.length > 0 && (
        <VStack gap={4} paddingBlockStart={6}>
          <Heading level={2}>You may also like</Heading>
          <ProductGrid
            hits={related}
            digits={country.minor_units}
            productBase={link(code, "/p")}
            minWidth={200}
          />
        </VStack>
      )}
    </VStack>
  );
}
