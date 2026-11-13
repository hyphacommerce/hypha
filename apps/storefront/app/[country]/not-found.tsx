"use client";

// A place nobody sells to: under the place's document, in the store's language. The
// shop's pages answer with the same page inside the header and footer
// (`(shop)/not-found.tsx`). A client component, since a not-found page isn't given the
// URL's params: the words and the home link come from the root layout's providers.

import { Button } from "@astryxdesign/core/Button";
import { EmptyState } from "@astryxdesign/core/EmptyState";
import { Section } from "@astryxdesign/core/Section";

import { useHome } from "@/components/locale";
import { useT } from "@/lib/use-t";

export default function NotFound() {
  const t = useT();
  return (
    <Section padding={10}>
      <EmptyState
        headingLevel={1}
        title={t("common.not_found_title")}
        description={t("common.not_found_description")}
        actions={<Button label={t("common.back_to_shop")} href={useHome()} variant="primary" />}
      />
    </Section>
  );
}
