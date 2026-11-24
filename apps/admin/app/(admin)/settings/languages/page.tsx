import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Suspense } from "react";

import { PageHeader } from "@/components/page-header";
import { CardSkeleton, FormSkeleton } from "@/components/skeletons";
import { requireStaff, requireStore, sessionClient } from "@/lib/hypha";
import { getT } from "@/lib/i18n";
import { settingsCrumb } from "@/lib/navigation";
import { canReadTranslations, translateHref } from "@/lib/translations";

import { LanguagesForm } from "./languages-form";
import { TranslationProgress } from "./progress";

/**
 * Settings → Languages: the language the store's content is written in, the languages
 * it's translated into, the way into translating the store's own text, and how far each
 * language has got.
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
        <Suspense fallback={<CardSkeleton />}>
          <TranslationProgress />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function Languages() {
  // The store is the layout's read too (`loadStore`): one call for both.
  // The markets, so a change one of them is in the way of is refused before it's
  // confirmed; none for a role without `markets:read` (the API's answer says so then).
  const [store, me, markets] = await Promise.all([
    requireStore(),
    requireStaff(),
    sessionClient().then((client) => client.GET("/admin/markets")),
  ]);
  return (
    <LanguagesForm
      // A saved change starts the form again from the store as it is now.
      key={JSON.stringify([store.default_language, store.languages])}
      defaultLanguage={store.default_language}
      languages={store.languages}
      markets={markets.data?.items.map(({ id, name, languages }) => ({ id, name, languages }))}
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
