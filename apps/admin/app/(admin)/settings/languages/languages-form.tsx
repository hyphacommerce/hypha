"use client";

// Settings → Languages: the content language, the languages the store is translated
// into (add, publish, remove), and the store's own text to translate. Each change is
// saved as it's made, always with the whole list, since the API replaces it.

import { AlertDialog } from "@astryxdesign/core/AlertDialog";
import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { IconButton } from "@astryxdesign/core/IconButton";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { Selector } from "@astryxdesign/core/Selector";
import { Switch } from "@astryxdesign/core/Switch";
import {
  Table,
  TableBody,
  TableCell,
  TableHeader,
  TableHeaderCell,
  TableRow,
} from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { TrashIcon } from "@heroicons/react/24/outline";
import { useState, useTransition } from "react";

import {
  COMMON_LANGUAGES,
  MAX_LANGUAGES,
  type StoreLanguage,
  canonicalTag,
  languageLabel,
  languagesInUse,
  offeredLanguages,
  sameTag,
} from "@/lib/languages";
import { useFormat } from "@/lib/use-format";
import { useT } from "@/lib/use-t";

import { type BlockingMarket, type LanguagesResult, saveLanguages } from "./actions";

export function LanguagesForm({
  defaultLanguage,
  languages,
  markets,
  canWrite,
  storeHref,
  facetsHref,
}: {
  defaultLanguage: string;
  languages: StoreLanguage[];
  /**
   * The markets with the languages each offers, to refuse a change one of them is in
   * the way of before it's confirmed; absent for a role that can't read markets, which
   * the API's refusal tells instead.
   */
  markets?: BlockingMarket[];
  /** Whether the role may change the store (`stores:write`). */
  canWrite: boolean;
  /** The editor for the store's own text, for a role that can open it. */
  storeHref?: string;
  /** The editor for the search filters' labels, likewise. */
  facetsHref?: string;
}) {
  const t = useT<"languages" | "errors">();
  const format = useFormat();
  const [failure, setFailure] = useState<LanguagesResult>({});
  const [isPending, startTransition] = useTransition();

  const [chosenDefault, setChosenDefault] = useState(defaultLanguage);
  const [common, setCommon] = useState<string>();
  const [typed, setTyped] = useState("");
  const [removing, setRemoving] = useState<StoreLanguage>();

  const name = (tag: string) => format.languageName(tag);
  const option = (tag: string) => ({ value: tag, label: languageLabel(format, tag) });
  const byLabel = (a: { label: string }, b: { label: string }) => format.compare(a.label, b.label);
  const isListed = (tag: string) =>
    sameTag(tag, defaultLanguage) || languages.some((l) => sameTag(l.language, tag));

  /** Sends a change, keeping what it answers; `true` when it went through. */
  const send = async (change: Parameters<typeof saveLanguages>[0]): Promise<boolean> => {
    const result = await saveLanguages(change);
    setFailure(result);
    return result.error === undefined;
  };

  type Change = Parameters<typeof saveLanguages>[0];
  /**
   * Whether markets still offer a language the change would take away: then they're
   * shown, as the API's refusal would show them, and the change isn't made (or
   * confirmed). The API checks again, for a market changed meanwhile.
   */
  const isBlocked = (change: Change): boolean => {
    if (!markets) return false;
    const inUse = languagesInUse(
      offeredLanguages(defaultLanguage, languages),
      offeredLanguages(change.defaultLanguage ?? defaultLanguage, change.languages ?? languages),
      markets,
    );
    if (inUse.length === 0) return false;
    setFailure({ error: t("errors.language_in_use"), markets: inUse });
    return true;
  };

  // The default language: a common tag, the stored one, or one of the store's others (a
  // typed tag among them).
  const defaultOptions = [
    ...new Set([defaultLanguage, ...languages.map((l) => l.language), ...COMMON_LANGUAGES]),
  ]
    .map(option)
    .toSorted(byLabel);
  const changeDefault = () => {
    // A listed language that becomes the default leaves the list in the same request.
    const rest = languages.filter((l) => !sameTag(l.language, chosenDefault));
    const change = {
      defaultLanguage: chosenDefault,
      languages: rest.length === languages.length ? undefined : rest,
    };
    if (isBlocked(change)) return;
    startTransition(async () => {
      await send(change);
    });
  };

  // Adding: a typed tag wins over the select, so "de-AT" can be written out.
  const typedTag = typed.trim() === "" ? undefined : canonicalTag(typed);
  const adding = typed.trim() === "" ? common : (typedTag ?? undefined);
  const isFull = languages.length >= MAX_LANGUAGES;
  const typedProblem =
    typed.trim() === ""
      ? undefined
      : typedTag === null
        ? t("languages.add_invalid")
        : typedTag !== undefined && isListed(typedTag)
          ? t("languages.add_listed")
          : undefined;
  const add = () => {
    if (!adding || isListed(adding) || isFull) return;
    startTransition(async () => {
      // New languages start unpublished: translate first, then show shoppers.
      if (await send({ languages: [...languages, { language: adding, published: false }] })) {
        setCommon(undefined);
        setTyped("");
      }
    });
  };

  // Each change sends the whole list built from the saved one, so a second change made
  // before the first's answer came back would undo it: every control waits while one is
  // in flight (the transition lasts until the page holds the saved list).
  const publish = (language: string, published: boolean) => {
    const change = {
      languages: languages.map((l) => (l.language === language ? { ...l, published } : l)),
    };
    if (isBlocked(change)) return;
    startTransition(async () => {
      await send(change);
    });
  };
  // Removing deletes translations for good: what's in its way is said before it's
  // confirmed, not after.
  const askToRemove = (language: StoreLanguage) => {
    if (isBlocked({ languages: languages.filter((l) => l.language !== language.language) }))
      return;
    setRemoving(language);
  };

  const remove = (language: string) =>
    startTransition(async () => {
      await send({ languages: languages.filter((l) => l.language !== language) });
      setRemoving(undefined);
    });

  return (
    <VStack gap={4}>
      {failure.error && (
        <Banner status="error" title={failure.error} collapsible={false}>
          {failure.markets && failure.markets.length > 0 && (
            <BlockingMarkets markets={failure.markets} />
          )}
        </Banner>
      )}
      <Card>
        <VStack gap={4}>
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              {t("languages.default_title")}
            </Text>
            <Text type="supporting" color="secondary">
              {t("languages.default_description")}
            </Text>
          </VStack>
          <HStack gap={3} vAlign="end" wrap="wrap">
            <Selector
              label={t("languages.default_label")}
              options={defaultOptions}
              value={chosenDefault}
              onChange={setChosenDefault}
              hasSearch
              isReadOnly={!canWrite}
              width={320}
            />
            {chosenDefault !== defaultLanguage && (
              <>
                <Button
                  label={t("languages.default_change")}
                  variant="primary"
                  isLoading={isPending}
                  onClick={changeDefault}
                />
                <Button
                  label={t("common.discard")}
                  variant="ghost"
                  isDisabled={isPending}
                  onClick={() => setChosenDefault(defaultLanguage)}
                />
              </>
            )}
          </HStack>
          {chosenDefault !== defaultLanguage && (
            <Text type="supporting" color="secondary">
              {t("languages.default_change_hint")}
            </Text>
          )}
        </VStack>
      </Card>

      <Card>
        <VStack gap={4}>
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              {t("languages.others_title")}
            </Text>
            <Text type="supporting" color="secondary">
              {t("languages.others_description")}
            </Text>
          </VStack>
          {languages.length === 0 ? (
            <Text color="secondary">{t("languages.others_empty")}</Text>
          ) : (
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHeaderCell>{t("common.language")}</TableHeaderCell>
                  <TableHeaderCell>{t("languages.column_published")}</TableHeaderCell>
                  <TableHeaderCell />
                </TableRow>
              </TableHeader>
              <TableBody>
                {languages.map((language) => (
                  <TableRow key={language.language}>
                    <TableCell>
                      <VStack gap={0}>
                        <HStack gap={2} vAlign="center">
                          <Text>{name(language.language)}</Text>
                          {!language.published && <Badge label={t("languages.unpublished")} />}
                        </HStack>
                        <Text type="supporting" color="secondary">
                          {language.language}
                        </Text>
                      </VStack>
                    </TableCell>
                    <TableCell>
                      <Switch
                        label={t("languages.publish_named", { name: name(language.language) })}
                        isLabelHidden
                        size="sm"
                        value={language.published}
                        isDisabled={!canWrite || isPending}
                        onChange={(published) => publish(language.language, published)}
                      />
                    </TableCell>
                    <TableCell>
                      {canWrite && (
                        <HStack gap={1} hAlign="end">
                          <IconButton
                            label={t("common.remove_named", { name: name(language.language) })}
                            icon={<TrashIcon />}
                            size="sm"
                            variant="ghost"
                            isDisabled={isPending}
                            onClick={() => askToRemove(language)}
                          />
                        </HStack>
                      )}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          )}
          {canWrite && (
            <VStack gap={2}>
              <HStack gap={3} vAlign="end" wrap="wrap">
                <Selector
                  label={t("languages.add_label")}
                  placeholder={t("languages.add_placeholder")}
                  options={COMMON_LANGUAGES.filter((tag) => !isListed(tag))
                    .map(option)
                    .toSorted(byLabel)}
                  value={common}
                  onChange={setCommon}
                  hasSearch
                  isDisabled={isFull || isPending || typed.trim() !== ""}
                  width={280}
                />
                <TextInput
                  label={t("languages.add_other_label")}
                  placeholder={t("languages.add_other_placeholder")}
                  value={typed}
                  onChange={setTyped}
                  onEnter={add}
                  isDisabled={isFull || isPending}
                  status={typedProblem ? { type: "error", message: typedProblem } : undefined}
                  width={200}
                />
                <Button
                  label={t("languages.add_action")}
                  isLoading={isPending && removing === undefined}
                  isDisabled={!adding || isListed(adding) || isFull || isPending}
                  onClick={add}
                />
              </HStack>
              <Text type="supporting" color="secondary">
                {isFull ? t("languages.add_full", { max: MAX_LANGUAGES }) : t("languages.add_hint")}
              </Text>
            </VStack>
          )}
        </VStack>
      </Card>

      {(storeHref || facetsHref) && (
        <Card>
          <VStack gap={4}>
            <VStack gap={1}>
              <Text type="display-3" as="h2">
                {t("languages.store_text_title")}
              </Text>
              <Text type="supporting" color="secondary">
                {t("languages.store_text_description")}
              </Text>
            </VStack>
            {languages.length === 0 ? (
              <Text color="secondary">{t("languages.store_text_empty")}</Text>
            ) : (
              <HStack gap={2} wrap="wrap">
                {storeHref && <Button label={t("languages.store_text_store")} href={storeHref} />}
                {facetsHref && (
                  <Button label={t("languages.store_text_facets")} href={facetsHref} />
                )}
              </HStack>
            )}
          </VStack>
        </Card>
      )}

      {removing && (
        <AlertDialog
          isOpen
          onOpenChange={(isOpen) => {
            if (!isOpen) setRemoving(undefined);
          }}
          title={t("languages.remove_title", { name: name(removing.language) })}
          description={t("languages.remove_description", { name: name(removing.language) })}
          actionLabel={t("languages.remove_action")}
          cancelLabel={t("common.cancel")}
          isActionLoading={isPending}
          onAction={() => remove(removing.language)}
        />
      )}
    </VStack>
  );
}

/** The markets still offering a language, each a link to where it's taken off. */
function BlockingMarkets({ markets }: { markets: BlockingMarket[] }) {
  const t = useT<"languages">();
  const format = useFormat();
  return (
    <VStack gap={2}>
      <Text type="supporting">{t("languages.in_use_hint")}</Text>
      {markets.map((market) => (
        <Text key={market.id} type="supporting">
          {t.rich("languages.in_use_market", {
            link: (chunks, key) => (
              <Link key={key} href={`/markets/${market.id}`}>
                {chunks}
              </Link>
            ),
            market: market.name,
            languages: format.list(
              market.languages.map((tag) => format.languageName(tag)),
              "conjunction",
            ),
          })}
        </Text>
      ))}
    </VStack>
  );
}
