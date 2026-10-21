"use client";

// The header's search (design note 0024, §7): suggestions as the shopper types (the
// first five results, through a server action, so the key stays on the server), and the
// first row searches everything for the words typed.

import { HStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import { Typeahead } from "@astryxdesign/core/Typeahead";
import type { SearchableItem, SearchSource } from "@astryxdesign/core/Typeahead";
import { MagnifyingGlassIcon } from "@heroicons/react/24/outline";
import { useRouter } from "next/navigation";
import { useMemo, useState } from "react";

import { suggest } from "@/app/[country]/actions";

import { useNavigationProgress } from "./navigation-progress";

type Item = SearchableItem<{ href: string }>;

export function SearchBox({ country, searchHref }: { country: string; searchHref: string }) {
  const router = useRouter();
  const { start } = useNavigationProgress();
  const [value, setValue] = useState<Item | null>(null);

  const source = useMemo<SearchSource<Item>>(
    () => ({
      bootstrap: () => [],
      async search(query) {
        const words = query.trim();
        const all: Item = {
          id: "search",
          label: `Search for “${words}”`,
          auxiliaryData: { href: `${searchHref}?q=${encodeURIComponent(words)}` },
        };
        const found = await suggest(country, words);
        return [
          all,
          ...found.map((s) => ({
            id: s.id,
            label: s.title,
            auxiliaryData: { href: s.href },
            element: (
              <HStack gap={3} vAlign="center" hAlign="between" width="100%">
                <HStack gap={2} vAlign="center">
                  {s.image ? (
                    <img
                      src={s.image}
                      alt=""
                      width={32}
                      height={32}
                      style={{ borderRadius: "var(--radius-inner)", objectFit: "cover" }}
                    />
                  ) : null}
                  <Text type="body">{s.title}</Text>
                </HStack>
                <Text type="supporting">{s.price}</Text>
              </HStack>
            ),
          })),
        ];
      },
    }),
    [country, searchHref],
  );

  return (
    <Typeahead
      label="Search"
      isLabelHidden
      placeholder="Search products"
      startIcon={MagnifyingGlassIcon}
      searchSource={source}
      debounceMs={200}
      maxMenuItems={6}
      width="100%"
      value={value}
      onChange={(item) => {
        setValue(null);
        if (item?.auxiliaryData) {
          // Not a link: the navigation bar is started by hand.
          start();
          router.push(item.auxiliaryData.href);
        }
      }}
    />
  );
}
