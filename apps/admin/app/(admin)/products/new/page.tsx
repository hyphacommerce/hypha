import { Section } from "@astryxdesign/core/Section";
import { VStack } from "@astryxdesign/core/Layout";

import { PageHeader } from "@/components/page-header";

import { NewProductForm } from "./new-product-form";

export default function NewProductPage() {
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={640}>
        <PageHeader
          title="Add product"
          description="A product with one variant, saved as a draft. Publish it when it's ready to sell."
          back={{ label: "Products", href: "/products" }}
        />
        <NewProductForm />
      </VStack>
    </Section>
  );
}
