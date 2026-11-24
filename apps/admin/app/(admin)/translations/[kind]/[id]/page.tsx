import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { notFound } from "next/navigation";
import { Suspense } from "react";

import { type Crumb, PageHeader } from "@/components/page-header";
import { DetailSkeleton } from "@/components/skeletons";
import { getFormat } from "@/lib/get-format";
import { loadStore, sessionClient } from "@/lib/hypha";
import { getT } from "@/lib/i18n";
import { languageLabel, sameTag } from "@/lib/languages";
import { type Kind, isKind, resourceHref } from "@/lib/translations";
import { loadTranslations, mayTranslate } from "@/lib/translations-server";

import { TranslationEditor } from "./translation-editor";

/**
 * The translation editor, one route for every kind of resource: a language, then each
 * field's source beside its translation. The way back renders at once; the fields
 * stream in.
 */
export default async function TranslationsPage({
  params,
  searchParams,
}: {
  params: Promise<{ kind: string; id: string }>;
  searchParams: Promise<Record<string, string | string[] | undefined>>;
}) {
  const [{ kind, id }, query, t] = await Promise.all([params, searchParams, getT()]);
  if (!isKind(kind)) notFound();
  const back: Crumb = { label: t(`translations.back_${kind}`), href: resourceHref(kind, id) };
  const asked = typeof query.language === "string" ? query.language : undefined;

  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={1120}>
        <Suspense fallback={<DetailSkeleton back={back} />}>
          <Translations kind={kind} id={id} asked={asked} back={back} />
        </Suspense>
      </VStack>
    </Section>
  );
}

/**
 * The option each of a product's option values belongs to, by the value's ID: the
 * translations name children by ID alone. Empty for other kinds, and for a role that
 * can't read the product (its values are then labelled without their option).
 */
async function optionNames(kind: Kind, id: string): Promise<Record<string, string>> {
  if (kind !== "product") return {};
  const client = await sessionClient();
  const { data } = await client.GET("/admin/products/{id}", { params: { path: { id } } });
  return Object.fromEntries(
    (data?.options ?? []).flatMap((option) => option.values.map((v) => [v.id, option.name])),
  );
}

async function Translations({
  kind,
  id,
  asked,
  back,
}: {
  kind: Kind;
  id: string;
  asked: string | undefined;
  back: Crumb;
}) {
  const [data, options, store, canWrite, t, format] = await Promise.all([
    loadTranslations(kind, id),
    optionNames(kind, id),
    // Which languages shoppers aren't served yet; unknown without `stores:read`.
    loadStore(),
    mayTranslate(kind),
    getT(),
    getFormat(),
  ]);
  // The resource's own name, where it has one among its fields.
  const named = data.fields.find((f) => f.key === "title" || f.key === "name")?.source.trim();
  const header = (
    <PageHeader
      title={named || t(`translations.kind_${kind}`)}
      back={back}
      description={t("translations.description", {
        language: languageLabel(format, data.default_language, "in_sentence"),
      })}
    />
  );

  if (data.languages.length === 0) {
    return (
      <>
        {header}
        <Banner
          status="info"
          title={t("translations.no_languages_title")}
          description={t("translations.no_languages_description")}
          endContent={
            <Button
              label={t("translations.no_languages_action")}
              variant="secondary"
              href="/settings/languages"
            />
          }
        />
      </>
    );
  }
  if (data.fields.length === 0) {
    return (
      <>
        {header}
        <Banner
          status="info"
          title={t("translations.no_fields_title")}
          description={t("translations.no_fields_description")}
        />
      </>
    );
  }

  // The API answers tags in canonical case; a link may not have written them so.
  const language =
    data.languages.find((tag) => asked !== undefined && sameTag(tag, asked)) ?? data.languages[0];
  const state = data.translations.find((s) => s.language === language);
  return (
    <>
      {header}
      <TranslationEditor
        // A new language, or a save of this one, starts a fresh draft from what's stored.
        key={`${language}|${state?.updated_at ?? ""}`}
        kind={kind}
        id={id}
        data={data}
        language={language}
        unpublished={(store?.languages ?? []).filter((l) => !l.published).map((l) => l.language)}
        options={options}
        canWrite={canWrite}
      />
    </>
  );
}
