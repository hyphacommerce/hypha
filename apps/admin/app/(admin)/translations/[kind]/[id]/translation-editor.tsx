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
  type TranslatableField,
  type Translations,
  byVariant,
  draftOf,
  grouped,
  isDirty,
  isSingleLine,
  isTooShort,
  isTranslatable,
  lengthOf,
  oneLine,
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
    case "custom_field":
      return name.name;
  }
}

type Section = ReturnType<typeof grouped<TranslatableField>>[number];

/** A section's parts: the custom fields by variant, any other section whole. */
function partsOf(section: Section) {
  return section.group === "custom_fields"
    ? byVariant(section.fields)
    : [{ variant: undefined, entries: section.fields }];
}

type PartVariant = { title: string | null } | undefined;

/**
 * Whether a section's parts get headings: whenever a variant's values are among them, so
 * a variant's value is never taken for the resource's own. Only products have variants,
 * so the own part's heading names a product.
 */
function hasPartHeadings(parts: readonly { variant?: PartVariant }[]): boolean {
  return parts.some((part) => part.variant !== undefined);
}

/** The heading of a custom fields part: the resource's own values, or a variant's. */
function partTitle(t: EditorT, variant: PartVariant): string {
  if (variant === undefined) return t("translations.custom_fields_own");
  return variant.title === null || variant.title.trim() === ""
    ? t("translations.custom_fields_variant_untitled")
    : t("translations.custom_fields_variant", { title: variant.title });
}

/**
 * A field's name for its input: a variant's custom field value names the variant too,
 * since the same definition's inputs repeat for each variant.
 */
function inputName(t: EditorT, name: FieldName, label: string): string {
  if (name.type !== "custom_field" || name.variant === undefined) return label;
  const title = name.variant.title?.trim();
  return t("translations.field_custom_field_variant", {
    name: label,
    variant: title || t("translations.custom_fields_variant_untitled"),
  });
}

/** A refused field's problem in the reader's language; the API's own for an unknown code. */
function problemText(t: EditorT, problem: FieldProblem, field: TranslatableField): string {
  const min = field.min_length;
  if (problem.code === "too_short" && min !== null && min !== undefined)
    return t("translations.problem_too_short_min", { min });
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
  // A field over its limit or under its minimum: the API would refuse the save.
  const isOutOfBounds = data.fields.some((field) => {
    if (!isTranslatable(field)) return false;
    const value = draft[field.key]?.value ?? "";
    return lengthOf(value, field.format) > field.max_length || isTooShort(value, field);
  });
  const saved = data.translations.find((state) => state.language === language);
  const sections = grouped(data.fields, (valueId) => options[valueId]);

  useEffect(() => {
    if (!isChanged) return;
    const warn = (event: BeforeUnloadEvent) => event.preventDefault();
    window.addEventListener("beforeunload", warn);
    return () => window.removeEventListener("beforeunload", warn);
  }, [isChanged]);

  const edit = (field: TranslatableField, change: Partial<DraftField>) => {
    // Whatever the translator does to a field, they did it against the source shown now.
    setDraft((before) => ({
      ...before,
      [field.key]: {
        value: "",
        reviewed: false,
        ...before[field.key],
        ...change,
        against: field.digest,
      },
    }));
    const key = field.key;
    setProblems(({ [key]: _fixed, ...rest }) => rest);
  };
  const discard = () => {
    setDraft(draftOf(data.fields, language));
    setError(undefined);
    setProblems({});
  };
  const save = () => {
    if (!isChanged || isOutOfBounds) return;
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

  // Shown alone in each field's label; the messages put it inside their sentence.
  const languageName = format.languageName(language);
  const inSentence = format.languageNameInSentence(language);

  return (
    <VStack gap={4}>
      <SaveBar
        isDirty={isChanged}
        isPending={isSaving}
        error={error}
        canSave={!isOutOfBounds}
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
                  total: data.fields.filter(isTranslatable).length,
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
          title={t("translations.unpublished_title", { language: inSentence })}
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
                {partsOf(section).map((part, _index, parts) => (
                  <VStack gap={4} key={part.variant?.id ?? "own"}>
                    {hasPartHeadings(parts) && (
                      <Text type="large" as="h3">
                        {partTitle(t, part.variant)}
                      </Text>
                    )}
                    {part.entries.map(({ field, name }) => (
                      <FieldRow
                        key={field.key}
                        t={t}
                        field={field}
                        name={name}
                        label={fieldLabel(t, name)}
                        sourceLanguage={data.default_language}
                        language={language}
                        languageName={languageName}
                        entry={draft[field.key] ?? { value: "", reviewed: false }}
                        problem={problems[field.key]}
                        canWrite={canWrite}
                        onChange={(change) => edit(field, change)}
                      />
                    ))}
                  </VStack>
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
          title={t("translations.remove_title", { language: inSentence })}
          description={t("translations.remove_description", { language: inSentence })}
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
  name,
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
  name: FieldName;
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
  const inputLabel = t("translations.translation_label", {
    field: inputName(t, name, label),
    language: languageName,
  });

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
      {isTranslatable(field) ? (
        <TranslationInput
          t={t}
          field={field}
          isOneLine={isSingleLine(field, name)}
          inputLabel={inputLabel}
          language={language}
          entry={entry}
          stored={stored}
          problem={problem}
          canWrite={canWrite}
          onChange={onChange}
        />
      ) : (
        // The API refuses a translation of empty source text (`source_empty`).
        <VStack gap={1}>
          <Text type="label">{inputLabel}</Text>
          <div className="translation-source">
            <Text color="secondary">{t("translations.not_translatable")}</Text>
          </div>
          {problem && (
            <span className="translation-problem">
              <Text type="supporting" color="inherit">
                {problemText(t, problem, field)}
              </Text>
            </span>
          )}
        </VStack>
      )}
    </div>
  );
}

/** A translatable field's input, its outdated state, its problem and its counter. */
function TranslationInput({
  t,
  field,
  isOneLine,
  inputLabel,
  language,
  entry,
  stored,
  problem,
  canWrite,
  onChange,
}: {
  t: EditorT;
  field: TranslatableField;
  /** A one-line input, which keeps line breaks out; a text area otherwise. */
  isOneLine: boolean;
  inputLabel: string;
  language: string;
  entry: DraftField;
  stored: TranslatableField["translations"][string] | undefined;
  problem: FieldProblem | undefined;
  canWrite: boolean;
  onChange: (change: Partial<DraftField>) => void;
}) {
  const isHtml = field.format === "html";
  const length = lengthOf(entry.value, field.format);
  const isOver = length > field.max_length;
  const isShort = isTooShort(entry.value, field);
  const min = field.min_length ?? undefined;
  // Edited text is saved against the source as shown, so only an untouched outdated
  // translation is left to mark by hand.
  const isUntouched = stored !== undefined && entry.value === stored.value;
  const status = problem || isOver || isShort ? ({ type: "error" } as const) : undefined;

  return (
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
        ) : isOneLine ? (
          <TextInput
            label={inputLabel}
            value={entry.value}
            onChange={(value) => onChange({ value: oneLine(value) })}
            isReadOnly={!canWrite}
            placeholder={canWrite ? undefined : t("translations.not_translated")}
            status={status}
          />
        ) : (
          <TextArea
            label={inputLabel}
            rows={4}
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
                {problemText(t, problem, field)}
              </Text>
            </span>
          )}
          {problem?.code === "source_changed" && canWrite && (
            <Button
              label={t("translations.keep_translation")}
              variant="ghost"
              size="sm"
              onClick={() => onChange({ reviewed: true })}
            />
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
          {!problem && !isOver && isShort && min !== undefined && (
            <span className="translation-problem">
              <Text type="supporting" color="inherit">
                {isHtml ? t("translations.too_short_html") : t("translations.too_short", { min })}
              </Text>
            </span>
          )}
        </HStack>
        {canWrite && !isHtml && (
          <span className={isOver || isShort ? "translation-problem" : undefined}>
            <Text type="supporting" color={isOver || isShort ? "inherit" : "secondary"}>
              {min === undefined
                ? t("translations.counter", { count: length, max: field.max_length })
                : t("translations.counter_min", { count: length, max: field.max_length, min })}
            </Text>
          </span>
        )}
      </HStack>
    </VStack>
  );
}

/** Stored HTML as it reads: the API sanitised it when it was saved. */
function Html({ html }: { html: string }) {
  // oxlint-disable-next-line react/no-danger -- sanitised by the API on every write
  return <div className="rich-text-source" dangerouslySetInnerHTML={{ __html: html }} />;
}
