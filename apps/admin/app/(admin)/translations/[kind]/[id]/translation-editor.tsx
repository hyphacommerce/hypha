"use client";

// The translation editor: a language, then each field's source on the left and its
// translation on the right. One draft per language, saved whole by the bar at the top.

import { AlertDialog } from "@astryxdesign/core/AlertDialog";
import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Selector } from "@astryxdesign/core/Selector";
import { Text } from "@astryxdesign/core/Text";
import { TextArea } from "@astryxdesign/core/TextArea";
import { TextInput } from "@astryxdesign/core/TextInput";
import { useRouter } from "next/navigation";
import { useEffect, useState, useTransition } from "react";

import { RichTextField } from "@/components/rich-text-field";
import { SaveBar } from "@/components/save-bar";
import { hasKey } from "@/lib/keys";
import { languageLabel } from "@/lib/languages";
import type { TFor } from "@/lib/messages";
import {
  type Draft,
  type DraftField,
  type FieldName,
  type FieldProblem,
  type Kind,
  SINGLE_LINE_MAX,
  type TranslatableField,
  type Translations,
  draftOf,
  grouped,
  isDirty,
  lengthOf,
  putBody,
  translateHref,
} from "@/lib/translations";
import { useFormat } from "@/lib/use-format";
import { useT } from "@/lib/use-t";

import { removeTranslation, saveTranslation } from "../../actions";

type EditorT = TFor<"translations" | "common">;

/** A field's label: by its name for a plain field, by its source text for a child. */
function fieldLabel(t: EditorT, name: FieldName): string {
  switch (name.type) {
    case "plain": {
      const key = `translations.field_${name.name}`;
      return hasKey(t, key) ? t(key) : name.name;
    }
    case "option":
      return t("translations.field_option", { name: name.source });
    case "option_value":
      return name.option === undefined
        ? t("translations.field_option_value_alone", { value: name.source })
        : t("translations.field_option_value", { option: name.option, value: name.source });
    case "choice":
      return t("translations.field_choice", { name: name.source });
    case "facet":
      return t("translations.field_facet", { name: name.source });
  }
}

/** A refused field's problem in the reader's language; the API's own for an unknown code. */
function problemText(t: EditorT, problem: FieldProblem): string {
  const key = `translations.problem_${problem.code}`;
  return hasKey(t, key) ? t(key) : problem.message || t("common.something_went_wrong");
}

export function TranslationEditor({
  kind,
  id,
  data,
  language,
  unpublished,
  options,
  canWrite,
}: {
  kind: Kind;
  id: string;
  data: Translations;
  /** The language being translated into: one of `data.languages`. */
  language: string;
  /** The store's languages shoppers aren't served yet. */
  unpublished: string[];
  /** A product's option names by option value ID. */
  options: Record<string, string>;
  /** Whether the role may save (`translations:write`); read-only otherwise. */
  canWrite: boolean;
}) {
  const t = useT<"translations">();
  const format = useFormat();
  const router = useRouter();
  const [draft, setDraft] = useState<Draft>(() => draftOf(data.fields, language));
  const [error, setError] = useState<string>();
  const [problems, setProblems] = useState<Record<string, FieldProblem>>({});
  const [isRemoving, setIsRemoving] = useState(false);
  const [isSaving, startSaving] = useTransition();
  const [isSwitching, startSwitching] = useTransition();

  const isChanged = canWrite && isDirty(data.fields, language, draft);
  const isOver = data.fields.some(
    (field) => lengthOf(draft[field.key]?.value ?? "", field.format) > field.max_length,
  );
  const saved = data.translations.find((state) => state.language === language);
  const sections = grouped(data.fields, (valueId) => options[valueId]);

  useEffect(() => {
    if (!isChanged) return;
    const warn = (event: BeforeUnloadEvent) => event.preventDefault();
    window.addEventListener("beforeunload", warn);
    return () => window.removeEventListener("beforeunload", warn);
  }, [isChanged]);

  const edit = (key: string, change: Partial<DraftField>) => {
    setDraft((before) => ({
      ...before,
      [key]: { value: "", reviewed: false, ...before[key], ...change },
    }));
    setProblems(({ [key]: _fixed, ...rest }) => rest);
  };
  const discard = () => {
    setDraft(draftOf(data.fields, language));
    setError(undefined);
    setProblems({});
  };
  const save = () => {
    if (!isChanged || isOver) return;
    startSaving(async () => {
      const result = await saveTranslation(
        kind,
        id,
        language,
        putBody(data.fields, language, draft),
      );
      setError(result.error);
      setProblems(result.problems ?? {});
    });
  };
  const remove = () =>
    startSaving(async () => {
      const result = await removeTranslation(kind, id, language);
      setError(result.error);
      setIsRemoving(false);
    });

  const languageName = format.languageName(language);

  return (
    <VStack gap={4}>
      <SaveBar
        isDirty={isChanged}
        isPending={isSaving}
        error={error}
        canSave={!isOver}
        onSave={save}
        onDiscard={discard}
      />
      <Card>
        <HStack gap={4} vAlign="end" hAlign="between" wrap="wrap">
          <HStack gap={3} vAlign="end" wrap="wrap">
            <Selector
              label={t("translations.language_label")}
              options={data.languages.map((tag) => ({
                value: tag,
                label: unpublished.includes(tag)
                  ? t("translations.language_unpublished", { language: languageLabel(format, tag) })
                  : languageLabel(format, tag),
              }))}
              value={language}
              onChange={(tag) =>
                // The same page in another language: replaced in place, never a push.
                startSwitching(() => router.replace(translateHref(kind, id, tag)))
              }
              hasSearch={data.languages.length > 8}
              isLoading={isSwitching}
              isDisabled={isChanged}
              disabledMessage={t("translations.language_locked")}
              width={320}
            />
            <VStack gap={0}>
              <Text type="supporting">
                {t("translations.progress", {
                  translated: saved?.translated ?? 0,
                  total: data.fields.length,
                })}
              </Text>
              {saved !== undefined && saved.outdated > 0 && (
                <Text type="supporting">
                  {t("translations.progress_outdated", { count: saved.outdated })}
                </Text>
              )}
            </VStack>
          </HStack>
          {canWrite && saved !== undefined && (
            <Button
              label={t("translations.remove_button")}
              variant="ghost"
              isDisabled={isSaving}
              onClick={() => setIsRemoving(true)}
            />
          )}
        </HStack>
      </Card>
      {unpublished.includes(language) && (
        <Banner
          status="info"
          title={t("translations.unpublished_title", { language: languageName })}
          description={t("translations.unpublished_description")}
        />
      )}
      {!canWrite && <Banner status="info" title={t("translations.read_only")} />}

      <div style={{ opacity: isSwitching ? 0.6 : 1 }}>
        <VStack gap={4}>
          {sections.map((section) => (
            <Card key={section.group}>
              <VStack gap={4}>
                {sections.length > 1 && (
                  <Text type="display-3" as="h2">
                    {t(`translations.group_${section.group}`)}
                  </Text>
                )}
                <div className="translation-row translation-row-head">
                  <Text type="supporting" weight="medium">
                    {languageLabel(format, data.default_language)}
                  </Text>
                  <Text type="supporting" weight="medium">
                    {languageLabel(format, language)}
                  </Text>
                </div>
                {section.fields.map(({ field, name }) => (
                  <FieldRow
                    key={field.key}
                    t={t}
                    field={field}
                    label={fieldLabel(t, name)}
                    sourceLanguage={data.default_language}
                    language={language}
                    languageName={languageName}
                    entry={draft[field.key] ?? { value: "", reviewed: false }}
                    problem={problems[field.key]}
                    canWrite={canWrite}
                    onChange={(change) => edit(field.key, change)}
                  />
                ))}
              </VStack>
            </Card>
          ))}
        </VStack>
      </div>

      {isRemoving && (
        <AlertDialog
          isOpen
          onOpenChange={(isOpen) => {
            if (!isOpen) setIsRemoving(false);
          }}
          title={t("translations.remove_title", { language: languageName })}
          description={t("translations.remove_description", { language: languageName })}
          actionLabel={t("translations.remove_action")}
          cancelLabel={t("common.cancel")}
          isActionLoading={isSaving}
          onAction={remove}
        />
      )}
    </VStack>
  );
}

/** One field: its source, read-only, beside its translation. */
function FieldRow({
  t,
  field,
  label,
  sourceLanguage,
  language,
  languageName,
  entry,
  problem,
  canWrite,
  onChange,
}: {
  t: EditorT;
  field: TranslatableField;
  label: string;
  sourceLanguage: string;
  language: string;
  languageName: string;
  entry: DraftField;
  problem: FieldProblem | undefined;
  canWrite: boolean;
  onChange: (change: Partial<DraftField>) => void;
}) {
  const stored = field.translations[language];
  const isHtml = field.format === "html";
  const length = lengthOf(entry.value, field.format);
  const isOver = length > field.max_length;
  // Edited text is saved against the source as shown, so only an untouched outdated
  // translation is left to mark by hand.
  const isUntouched = stored !== undefined && entry.value === stored.value;
  const inputLabel = t("translations.translation_label", { field: label, language: languageName });
  const status = problem || isOver ? ({ type: "error" } as const) : undefined;

  return (
    <div className="translation-row">
      <VStack gap={1}>
        <Text type="label">{label}</Text>
        <div className="translation-source" lang={sourceLanguage}>
          {field.source.trim() === "" ? (
            <Text color="secondary">{t("translations.source_empty")}</Text>
          ) : isHtml ? (
            <Html html={field.source} />
          ) : (
            <Text>{field.source}</Text>
          )}
        </div>
      </VStack>
      <VStack gap={1}>
        <div lang={language}>
          {!canWrite && isHtml ? (
            <VStack gap={1}>
              <Text type="label">{inputLabel}</Text>
              <div className="translation-source">
                {entry.value === "" ? (
                  <Text color="secondary">{t("translations.not_translated")}</Text>
                ) : (
                  <Html html={entry.value} />
                )}
              </div>
            </VStack>
          ) : isHtml ? (
            <RichTextField
              label={inputLabel}
              value={entry.value}
              onChange={(value) => onChange({ value })}
              isOptional
            />
          ) : field.max_length > SINGLE_LINE_MAX ? (
            <TextArea
              label={inputLabel}
              rows={4}
              value={entry.value}
              onChange={(value) => onChange({ value })}
              isReadOnly={!canWrite}
              placeholder={canWrite ? undefined : t("translations.not_translated")}
              status={status}
            />
          ) : (
            <TextInput
              label={inputLabel}
              value={entry.value}
              onChange={(value) => onChange({ value })}
              isReadOnly={!canWrite}
              placeholder={canWrite ? undefined : t("translations.not_translated")}
              status={status}
            />
          )}
        </div>
        <HStack gap={2} vAlign="center" hAlign="between" wrap="wrap">
          <HStack gap={2} vAlign="center" wrap="wrap">
            {stored?.outdated && isUntouched && !entry.reviewed && (
              <>
                <Badge label={t("translations.outdated")} variant="warning" />
                {canWrite && (
                  <Button
                    label={t("translations.mark_reviewed")}
                    tooltip={t("translations.mark_reviewed_tooltip")}
                    variant="ghost"
                    size="sm"
                    onClick={() => onChange({ reviewed: true })}
                  />
                )}
              </>
            )}
            {stored?.outdated && isUntouched && entry.reviewed && (
              <>
                <Badge label={t("translations.reviewed")} variant="success" />
                <Button
                  label={t("translations.undo_reviewed")}
                  variant="ghost"
                  size="sm"
                  onClick={() => onChange({ reviewed: false })}
                />
              </>
            )}
            {problem && (
              <span className="translation-problem">
                <Text type="supporting" color="inherit">
                  {problemText(t, problem)}
                </Text>
              </span>
            )}
            {!problem && isOver && (
              <span className="translation-problem">
                <Text type="supporting" color="inherit">
                  {isHtml
                    ? t("translations.too_long_html")
                    : t("translations.too_long", { max: field.max_length })}
                </Text>
              </span>
            )}
          </HStack>
          {canWrite && !isHtml && (
            <span className={isOver ? "translation-problem" : undefined}>
              <Text type="supporting" color={isOver ? "inherit" : "secondary"}>
                {t("translations.counter", { count: length, max: field.max_length })}
              </Text>
            </span>
          )}
        </HStack>
      </VStack>
    </div>
  );
}

/** Stored HTML as it reads: the API sanitised it when it was saved. */
function Html({ html }: { html: string }) {
  // oxlint-disable-next-line react/no-danger -- sanitised by the API on every write
  return <div className="rich-text-source" dangerouslySetInnerHTML={{ __html: html }} />;
}
