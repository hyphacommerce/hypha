"use client";

// Go anywhere from the keyboard: ⌘K (Ctrl+K elsewhere) opens a palette of every page in
// the nav, every settings page and the common "new …" pages.

import { CommandPalette } from "@astryxdesign/core/CommandPalette";
import { IconButton } from "@astryxdesign/core/IconButton";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Text } from "@astryxdesign/core/Text";
import { type SearchableItem, createStaticSource } from "@astryxdesign/core/Typeahead";
import { MagnifyingGlassIcon, PlusIcon } from "@heroicons/react/24/outline";
import { useRouter } from "next/navigation";
import { useEffect, useMemo, useState } from "react";

import {
  ACCOUNT_NAV,
  type NavIcon,
  canOpen,
  visibleSections,
  visibleSettings,
} from "@/lib/navigation";
import type { SharedNamespace, TFor } from "@/lib/messages";
import { useT } from "@/lib/use-t";

type Command = SearchableItem<{ group: string; icon?: NavIcon; hint?: string; keywords: string[] }>;

function command(
  group: string,
  label: string,
  href: string,
  icon?: NavIcon,
  hint?: string,
  keywords: string[] = [],
): Command {
  return { id: href, label, auxiliaryData: { group, icon, hint, keywords } };
}

/** Every page `scopes` can open, named in `t`'s language. */
function commands(scopes: readonly string[], t: TFor<SharedNamespace>): Command[] {
  const goTo = t("shell.command_group_go_to");
  return [
    ...visibleSections(scopes)
      .flatMap((section) => section.items)
      .flatMap((item) => [
        command(goTo, t(item.label), item.href, item.icon),
        ...(item.children ?? []).map((child) =>
          command(goTo, t(child.label), child.href, item.icon, t(item.label)),
        ),
      ]),
    command(
      goTo,
      t(ACCOUNT_NAV.label),
      ACCOUNT_NAV.href,
      ACCOUNT_NAV.icon,
      undefined,
      // Words someone might type for the page, separated by commas in the catalog.
      t("shell.command_account_keywords").split(","),
    ),
    ...visibleSettings(scopes).flatMap((group) =>
      group.pages.map((page) =>
        command(
          t("shell.command_group_settings"),
          t(page.label),
          page.href,
          page.icon,
          t(group.title),
          [t(group.title), t(page.description)],
        ),
      ),
    ),
    ...[
      { label: t("shell.command_new_product"), href: "/products/new", scope: "products:write" },
      { label: t("shell.command_new_market"), href: "/markets/new", scope: "markets:write" },
    ]
      .filter((page) => canOpen(page, scopes))
      .map((page) => command(t("shell.command_group_create"), page.label, page.href, PlusIcon)),
  ];
}

function CommandRow({ item }: { item: Command }) {
  const Icon = item.auxiliaryData?.icon;
  return (
    <HStack gap={2} vAlign="center">
      {Icon && <Icon width={16} aria-hidden />}
      <VStack gap={0}>
        <Text>{item.label}</Text>
        {item.auxiliaryData?.hint && (
          <Text type="supporting" color="secondary">
            {item.auxiliaryData.hint}
          </Text>
        )}
      </VStack>
    </HStack>
  );
}

export function CommandMenu({ scopes }: { scopes: readonly string[] }) {
  const [isOpen, setIsOpen] = useState(false);
  const router = useRouter();
  const t = useT();
  const source = useMemo(
    () =>
      createStaticSource(commands(scopes, t), {
        keywords: (item) => item.auxiliaryData?.keywords ?? [],
      }),
    [scopes, t],
  );

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key.toLowerCase() === "k" && (event.metaKey || event.ctrlKey)) {
        event.preventDefault();
        setIsOpen((open) => !open);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <>
      <IconButton
        label={t("shell.command_open")}
        tooltip={t("shell.command_tooltip")}
        variant="ghost"
        size="sm"
        icon={<MagnifyingGlassIcon width={18} />}
        onClick={() => setIsOpen(true)}
      />
      {isOpen && (
        <CommandPalette
          isOpen
          onOpenChange={setIsOpen}
          label={t("shell.command_group_go_to")}
          searchSource={source}
          emptyBootstrapText={t("shell.command_empty")}
          renderItem={(item) => <CommandRow item={item} />}
          onValueChange={(href) => {
            setIsOpen(false);
            router.push(href);
          }}
        />
      )}
    </>
  );
}
