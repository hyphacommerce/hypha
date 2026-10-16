"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { Divider } from "@astryxdesign/core/Divider";
import { IconButton } from "@astryxdesign/core/IconButton";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Selector } from "@astryxdesign/core/Selector";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { Tokenizer } from "@astryxdesign/core/Tokenizer";
import type { SearchableItem, SearchSource } from "@astryxdesign/core/Typeahead";
import type { AdminSchemas } from "@hypha/sdk/admin";
import { ArrowDownIcon, ArrowUpIcon, TrashIcon } from "@heroicons/react/24/outline";
import { useState, useTransition } from "react";

import { saveSearchSettings } from "./actions";

type Settings = AdminSchemas["SearchSettingsResponse"];

/** What a filter counts: a product field, or an option by name. */
type Kind = "product_type" | "vendor" | "tag" | "option";

const KINDS: { value: Kind; label: string }[] = [
  { value: "product_type", label: "Product type" },
  { value: "vendor", label: "Vendor" },
  { value: "tag", label: "Tag" },
  { value: "option", label: "Option" },
];

/** A filter being edited; `key` is stable for React. */
type Draft = { key: string; kind: Kind; option: string; label: string };

let nextKey = 0;
const newKey = () => `facet-${nextKey++}`;

/** `option.color` → an option draft named "color"; the other keys are their kind. */
function draft(facet: Settings["facets"][number]): Draft {
  const option = facet.key.startsWith("option.") ? facet.key.slice("option.".length) : "";
  const kind = (option ? "option" : facet.key) as Kind;
  return { key: newKey(), kind, option, label: facet.label ?? "" };
}

/** The facet key a draft saves as (`option.<name>`), or `null` for an unnamed option. */
function facetKey(d: Draft): string | null {
  if (d.kind !== "option") return d.kind;
  const name = d.option.trim();
  return name ? `option.${name}` : null;
}

/** Options are typed in (any product's option name), not picked: `hasCreate` adds. */
const noSuggestions: SearchSource = { search: () => [], bootstrap: () => [] };
const asItems = (names: string[]): SearchableItem[] => names.map((n) => ({ id: n, label: n }));

/**
 * Listings and filters (design note 0019, section 6), saved together. Changing which
 * options split listings rebuilds every search document in the background; filters
 * apply at once.
 */
export function SearchSettingsForm({ settings }: { settings: Settings }) {
  const [split, setSplit] = useState(asItems(settings.split_by));
  const [facets, setFacets] = useState<Draft[]>(() => settings.facets.map(draft));
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();

  const request = () => ({
    split_by: split.map((s) => s.label),
    facets: facets.map((f) => ({ key: facetKey(f) ?? "", label: f.label.trim() || null })),
  });
  const saved = JSON.stringify({
    split_by: settings.split_by,
    facets: settings.facets.map((f) => ({ key: f.key, label: f.label ?? null })),
  });
  const isDirty = JSON.stringify(request()) !== saved;
  const unnamed = facets.some((f) => facetKey(f) === null);

  const update = (key: string, change: Partial<Draft>) =>
    setFacets((before) => before.map((f) => (f.key === key ? { ...f, ...change } : f)));
  const move = (from: number, to: number) =>
    setFacets((before) => {
      const next = [...before];
      const [moved] = next.splice(from, 1);
      next.splice(to, 0, moved);
      return next;
    });

  const save = () =>
    startTransition(async () => {
      const result = await saveSearchSettings(request());
      setError(result.error);
    });
  const reset = () => {
    setSplit(asItems(settings.split_by));
    setFacets(settings.facets.map(draft));
    setError(undefined);
  };

  return (
    <Card>
      <VStack gap={5}>
        {error && <Banner status="error" title={error} />}

        <VStack gap={2}>
          <Text type="display-3" as="h2">
            Listings
          </Text>
          <Tokenizer
            label="Show each value as its own listing"
            description="Option names, up to 3. With “Color”, a shirt in red and blue shows as two results, each with its sizes. Changing this rebuilds search in the background."
            isOptional
            placeholder="Add an option name…"
            searchSource={noSuggestions}
            hasCreate
            debounceMs={0}
            maxEntries={3}
            value={split}
            onChange={(items) =>
              setSplit(
                items.filter(
                  (item, i) =>
                    items.findIndex((o) => o.label.toLowerCase() === item.label.toLowerCase()) ===
                    i,
                ),
              )
            }
          />
        </VStack>

        <Divider />

        <VStack gap={3}>
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              Filters
            </Text>
            <Text type="supporting" color="secondary">
              What shoppers can narrow results by, in this order. With none, storefronts show
              product type, vendor, the most used options and tags.
            </Text>
          </VStack>
          {facets.map((f, i) => (
            <HStack key={f.key} gap={2} vAlign="end" wrap="wrap">
              <Selector
                label="Filter by"
                isLabelHidden={i > 0}
                options={KINDS}
                value={f.kind}
                onChange={(kind) => update(f.key, { kind: kind as Kind })}
                width={160}
              />
              {f.kind === "option" && (
                <TextInput
                  label="Option name"
                  isLabelHidden={i > 0}
                  placeholder="Its name on products"
                  value={f.option}
                  onChange={(option) => update(f.key, { option })}
                  width={160}
                />
              )}
              <TextInput
                label="Label"
                isLabelHidden={i > 0}
                isOptional
                placeholder="As the storefront names it"
                value={f.label}
                onChange={(label) => update(f.key, { label })}
                width={200}
              />
              <HStack gap={1}>
                <IconButton
                  label="Move up"
                  icon={<ArrowUpIcon width={16} />}
                  variant="ghost"
                  isDisabled={i === 0}
                  onClick={() => move(i, i - 1)}
                />
                <IconButton
                  label="Move down"
                  icon={<ArrowDownIcon width={16} />}
                  variant="ghost"
                  isDisabled={i === facets.length - 1}
                  onClick={() => move(i, i + 1)}
                />
                <IconButton
                  label="Remove"
                  icon={<TrashIcon width={16} />}
                  variant="ghost"
                  onClick={() => setFacets((before) => before.filter((x) => x.key !== f.key))}
                />
              </HStack>
            </HStack>
          ))}
          <HStack>
            <Button
              label="Add filter"
              variant="ghost"
              isDisabled={facets.length >= 20}
              onClick={() =>
                setFacets((before) => [
                  ...before,
                  { key: newKey(), kind: "option", option: "", label: "" },
                ])
              }
            />
          </HStack>
        </VStack>

        <HStack gap={2} hAlign="end" vAlign="center">
          {unnamed && (
            <Text type="supporting" color="secondary">
              Name every option filter to save.
            </Text>
          )}
          {isDirty && <Button label="Discard" variant="ghost" onClick={reset} />}
          <Button
            label="Save"
            variant="primary"
            isDisabled={!isDirty || unnamed}
            isLoading={isPending}
            onClick={save}
          />
        </HStack>
      </VStack>
    </Card>
  );
}
