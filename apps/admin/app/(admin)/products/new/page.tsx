import { Section } from "@astryxdesign/core/Section";
import { VStack } from "@astryxdesign/core/Layout";

import { PageHeader } from "@/components/page-header";
import { getT } from "@/lib/i18n";

import { NewProductForm } from "./new-product-form";

export default async function NewProductPage() {
  const t = await getT();
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={640}>
        <PageHeader
          title={t("products.add_product")}
          description={t("products.new_description")}
          back={{ label: t("common.products"), href: "/products" }}
        />
        <NewProductForm />
      </VStack>
    </Section>
  );
}
