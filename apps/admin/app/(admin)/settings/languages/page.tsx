import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Suspense } from "react";

import { PageHeader } from "@/components/page-header";
import { FormSkeleton } from "@/components/skeletons";
import { requireStaff, requireStore } from "@/lib/hypha";
import { getT } from "@/lib/i18n";
import { settingsCrumb } from "@/lib/navigation";
import { canReadTranslations, translateHref } from "@/lib/translations";

import { LanguagesForm } from "./languages-form";

/**
 * Settings → Languages: the language the store's content is written in, the languages
 * it's translated into, and the way into translating the store's own text.
 */
export default async function LanguagesSettingsPage() {
  const t = await getT();
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={720}>
        <PageHeader
          back={settingsCrumb(t)}
          title={t("languages.title")}
          description={t("languages.description")}
        />
        <Suspense fallback={<FormSkeleton />}>
          <Languages />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function Languages() {
  // The store is the layout's read too (`loadStore`): one call for both.
  const [store, me] = await Promise.all([requireStore(), requireStaff()]);
  return (
    <LanguagesForm
      // A saved change starts the form again from the store as it is now.
      key={JSON.stringify([store.default_language, store.languages])}
      defaultLanguage={store.default_language}
      languages={store.languages}
      canWrite={me.scopes.includes("stores:write")}
      storeHref={
        canReadTranslations(me.scopes, "store") ? translateHref("store", store.id) : undefined
      }
      facetsHref={
        canReadTranslations(me.scopes, "search_facet")
          ? translateHref("search_facet", store.id)
          : undefined
      }
    />
  );
}
