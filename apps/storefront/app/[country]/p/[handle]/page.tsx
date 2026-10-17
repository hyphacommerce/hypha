import { Banner } from "@astryxdesign/core/Banner";
import { BreadcrumbItem, Breadcrumbs } from "@astryxdesign/core/Breadcrumbs";
import { Heading } from "@astryxdesign/core/Heading";
import { VStack } from "@astryxdesign/core/Stack";
import type { Metadata } from "next";
import { notFound } from "next/navigation";

import { countryOf, loadProduct } from "@/lib/api";
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
  const country = await countryOf(folder);
  const code = country.country;
  const product = await load(handle, code);

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
        <ProductForm product={product} country={code} digits={country.minor_units} />
      )}
    </VStack>
  );
}
