"use client";

import { Badge } from "@astryxdesign/core/Badge";
import { Card } from "@astryxdesign/core/Card";
import { IconButton } from "@astryxdesign/core/IconButton";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { Selector } from "@astryxdesign/core/Selector";
import { Text } from "@astryxdesign/core/Text";
import { ArrowDownIcon, ArrowUpIcon, XMarkIcon } from "@heroicons/react/24/outline";
import type { ReactNode } from "react";

import { addedLanguage as added, languageLabel, moved } from "@/lib/languages";
import { useFormat } from "@/lib/use-format";
import { useT } from "@/lib/use-t";

/** The link to where the store's languages are managed, for a sentence that names it. */
function settingsLink(chunks: ReactNode, key: string | number) {
  return (
    <Link key={key} href="/settings/languages">
      {chunks}
    </Link>
  );
}

/**
 * The languages a market offers its shoppers, in order: the first is the market's
 * default. Part of the market's draft, saved by the page's bar. An empty list means the
 * store's default language alone.
 */
export function LanguagesCard({
  storeDefault,
  offered,
  value,
  onChange,
}: {
  /** The store's default language: what an empty list stands for. */
  storeDefault: string;
  /** What a market can offer: the store's default and its published languages. */
  offered: string[];
  value: string[];
  onChange: (languages: string[]) => void;
}) {
  const t = useT<"markets">();
  const format = useFormat();
  const remaining = offered.filter((tag) => !value.includes(tag));

  return (
    <Card>
      <VStack gap={4}>
        <VStack gap={1}>
          <Text type="display-3" as="h2">
            {t("markets.languages_title")}
          </Text>
          <Text type="supporting" color="secondary">
            {t("markets.languages_description")}
          </Text>
        </VStack>
        {value.length === 0 ? (
          <Text color="secondary">
            {t("markets.languages_empty", { language: languageLabel(format, storeDefault) })}
          </Text>
        ) : (
          <VStack gap={2}>
            {value.map((tag, index) => {
              const name = format.languageName(tag);
              return (
                <HStack key={tag} gap={2} vAlign="center" hAlign="between">
                  <HStack gap={2} vAlign="center" wrap="wrap">
                    <Text>{name}</Text>
                    <Text type="supporting" color="secondary">
                      {tag}
                    </Text>
                    {index === 0 && <Badge label={t("markets.languages_default_badge")} />}
                  </HStack>
                  <HStack gap={1}>
                    <IconButton
                      label={t("common.move_up_named", { name })}
                      icon={<ArrowUpIcon />}
                      size="sm"
                      variant="ghost"
                      isDisabled={index === 0}
                      onClick={() => onChange(moved(value, index, -1))}
                    />
                    <IconButton
                      label={t("common.move_down_named", { name })}
                      icon={<ArrowDownIcon />}
                      size="sm"
                      variant="ghost"
                      isDisabled={index === value.length - 1}
                      onClick={() => onChange(moved(value, index, 1))}
                    />
                    <IconButton
                      label={t("common.remove_named", { name })}
                      icon={<XMarkIcon />}
                      size="sm"
                      variant="ghost"
                      onClick={() => onChange(value.filter((other) => other !== tag))}
                    />
                  </HStack>
                </HStack>
              );
            })}
          </VStack>
        )}
        {remaining.length > 0 && (
          <Selector
            // A pick is added to the list at once, so the select starts empty again.
            key={value.join("|")}
            label={t("markets.languages_add_label")}
            placeholder={t("markets.languages_add_placeholder")}
            options={remaining.map((tag) => ({ value: tag, label: languageLabel(format, tag) }))}
            onChange={(tag) => onChange(added(value, tag, storeDefault))}
            hasSearch={remaining.length > 8}
          />
        )}
        <Text type="supporting" color="secondary">
          {offered.length <= 1
            ? t.rich("markets.languages_none_published", { link: settingsLink })
            : t.rich("markets.languages_hint", { link: settingsLink })}
        </Text>
      </VStack>
    </Card>
  );
}
