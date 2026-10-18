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
  MAIN_NAV,
  type NavIcon,
  SETTINGS_NAV,
  canOpen,
  visibleNav,
  visibleSettings,
} from "@/lib/navigation";

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

/** Every page `scopes` can open (all while they're unknown, `null`). */
function commands(scopes: readonly string[] | null): Command[] {
  return [
    ...visibleNav(MAIN_NAV, scopes).flatMap((item) => [
      command("Go to", item.label, item.href, item.icon),
      ...(item.children ?? []).map((child) =>
        command("Go to", child.label, child.href, item.icon, item.label),
      ),
    ]),
    command("Go to", SETTINGS_NAV.label, SETTINGS_NAV.href, SETTINGS_NAV.icon),
    command("Go to", ACCOUNT_NAV.label, ACCOUNT_NAV.href, ACCOUNT_NAV.icon, undefined, [
      "password",
      "two-factor",
      "2fa",
      "profile",
    ]),
    ...visibleSettings(scopes).flatMap((group) =>
      group.pages.map((page) =>
        command("Settings", page.label, page.href, page.icon, group.title, [
          group.title,
          page.description,
        ]),
      ),
    ),
    ...[
      { label: "New product", href: "/products/new", scope: "products:write" },
      { label: "New market", href: "/markets/new", scope: "markets:write" },
    ]
      .filter((page) => canOpen(page, scopes))
      .map((page) => command("Create", page.label, page.href, PlusIcon)),
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

export function CommandMenu({ scopes = null }: { scopes?: readonly string[] | null }) {
  const [isOpen, setIsOpen] = useState(false);
  const router = useRouter();
  const source = useMemo(
    () =>
      createStaticSource(commands(scopes), {
        keywords: (item) => item.auxiliaryData?.keywords ?? [],
      }),
    [scopes],
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
        label="Search the admin"
        tooltip="Go to… (⌘K)"
        variant="ghost"
        size="sm"
        icon={<MagnifyingGlassIcon width={18} />}
        onClick={() => setIsOpen(true)}
      />
      {isOpen && (
        <CommandPalette
          isOpen
          onOpenChange={setIsOpen}
          label="Go to"
          searchSource={source}
          emptyBootstrapText="Type a page's name"
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
