import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { forbidden } from "next/navigation";
import { Suspense } from "react";

import { type Crumb, PageHeader } from "@/components/page-header";
import { type SearchParams, one } from "@/components/resource-table/params";
import { listPage, pageQuery } from "@/components/resource-table/server";
import { loadStore, loaded, requireStaff, sessionClient } from "@/lib/hypha";
import { getT } from "@/lib/i18n";
import type { T } from "@/lib/messages";
import { hasScope, settingsCrumb } from "@/lib/navigation";
import {
  WORKLIST_PATH,
  type WorklistFilters,
  worklistFilters,
  worklistQuery,
} from "@/lib/translation-worklist";
import { resourceNames } from "@/lib/translation-worklist-server";
import { KINDS, type Kind, canReadTranslations } from "@/lib/translations";

import { WorklistControls } from "./worklist-controls";
import { WorklistTable } from "./worklist-table";

type Language = { language: string; published?: boolean };

/**
 * What's left to translate into one of the store's languages: every resource with text
 * in the chosen state (missing, outdated or complete), with its counts and the way into
 * its editor. The header, the language and state and the table's toolbar render at once;
 * the rows stream in. Without `stores:read` the languages aren't known up front, and the
 * choices stream in with the rows.
 */
export default async function TranslationWorklistPage({
  searchParams,
}: {
  searchParams: Promise<SearchParams>;
}) {
  const [params, me, store, t] = await Promise.all([
    searchParams,
    requireStaff(),
    loadStore(),
    getT(),
  ]);
  if (!hasScope(me.scopes, "translations:read")) forbidden();
  const kinds = KINDS.filter((kind) => canReadTranslations(me.scopes, kind));
  const back: Crumb[] = [
    settingsCrumb(t),
    { label: t("translations.worklist_back"), href: "/settings/languages" },
  ];
  const header = (
    <PageHeader
      back={back}
      title={t("translations.worklist_title")}
      description={t("translations.worklist_description")}
    />
  );

  if (store === null) {
    return (
      <Section padding={6}>
        <VStack gap={5}>
          {header}
          <Suspense fallback={<WorklistTable kinds={kinds} language="" state="missing" />}>
            <LanguagesThenRows params={params} kinds={kinds} />
          </Suspense>
        </VStack>
      </Section>
    );
  }
  const languages = store.languages;
  const filters = filtersFor(params, languages, kinds);
  return (
    <Section padding={6}>
      <VStack gap={5}>
        {header}
        <Worklist languages={languages} filters={filters} params={params} kinds={kinds} t={t} />
      </VStack>
    </Section>
  );
}

/** The filters from the URL, with a kind the role can't read dropped. */
function filtersFor(
  params: SearchParams,
  languages: readonly Language[],
  kinds: readonly Kind[],
): WorklistFilters {
  const filters = worklistFilters(
    { language: one(params, "language"), state: one(params, "state"), kind: one(params, "kind") },
    languages.map((l) => l.language),
  );
  return filters.kind && kinds.includes(filters.kind) ? filters : { ...filters, kind: undefined };
}

/** The choices above the table and the table, or a note when there's nothing to choose. */
function Worklist({
  languages,
  filters,
  params,
  kinds,
  t,
}: {
  languages: Language[];
  filters: WorklistFilters;
  params: SearchParams;
  kinds: Kind[];
  t: T;
}) {
  const { language, state } = filters;
  if (language === undefined) {
    return (
      <Banner
        status="info"
        title={t("translations.worklist_no_languages_title")}
        description={t("translations.worklist_no_languages_description")}
        endContent={
          <Button
            label={t("translations.worklist_no_languages_action")}
            variant="secondary"
            href="/settings/languages"
          />
        }
      />
    );
  }
  return (
    <WorklistControls languages={languages} language={language} state={state} kind={filters.kind}>
      <Suspense fallback={<WorklistTable kinds={kinds} language={language} state={state} />}>
        <Rows filters={{ ...filters, language }} params={params} kinds={kinds} />
      </Suspense>
    </WorklistControls>
  );
}

/** Without `stores:read`: the languages from the progress read, then the worklist. */
async function LanguagesThenRows({ params, kinds }: { params: SearchParams; kinds: Kind[] }) {
  const [client, t] = await Promise.all([sessionClient(), getT()]);
  const progress = loaded(await client.GET("/admin/translations/progress"));
  const languages = progress.languages.map((l) => ({ language: l.language }));
  return (
    <Worklist
      languages={languages}
      filters={filtersFor(params, languages, kinds)}
      params={params}
      kinds={kinds}
      t={t}
    />
  );
}

async function Rows({
  filters,
  params,
  kinds,
}: {
  filters: WorklistFilters & { language: string };
  params: SearchParams;
  kinds: Kind[];
}) {
  const [client, t] = await Promise.all([sessionClient(), getT()]);
  const result = await client.GET("/admin/translations", {
    params: { query: worklistQuery(filters, pageQuery(params)) },
  });
  const page = await listPage(result, WORKLIST_PATH, params);
  const [names, uncounted] = await Promise.all([
    resourceNames(page.items, t),
    page.items.length === 0 && !page.error ? isUncountedFor(filters) : false,
  ]);
  return (
    <WorklistTable
      page={page}
      names={names}
      kinds={kinds}
      language={filters.language}
      state={filters.state}
      isUncounted={uncounted}
    />
  );
}

/**
 * Whether an empty list is empty because nothing is counted yet (the API counts the
 * store's texts in the background after an upgrade), not because nothing is left: then
 * the table says so rather than "Nothing left to translate".
 */
async function isUncountedFor(filters: WorklistFilters & { language: string }): Promise<boolean> {
  const client = await sessionClient();
  const { data } = await client.GET("/admin/translations/progress", {
    params: { query: filters.kind ? { kind: filters.kind } : {} },
  });
  const progress = data?.languages.find((l) => l.language === filters.language);
  return progress !== undefined && progress.total === 0;
}
