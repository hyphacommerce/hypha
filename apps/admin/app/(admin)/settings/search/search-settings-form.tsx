"use client";

import { Badge } from "@astryxdesign/core/Badge";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { CheckboxInput } from "@astryxdesign/core/CheckboxInput";
import { Divider } from "@astryxdesign/core/Divider";
import { IconButton } from "@astryxdesign/core/IconButton";
import { HStack, StackItem, VStack } from "@astryxdesign/core/Layout";
import { Switch } from "@astryxdesign/core/Switch";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { ArrowDownIcon, ArrowUpIcon, PlusIcon, XMarkIcon } from "@heroicons/react/24/outline";
import { type ReactNode, useState, useTransition } from "react";

import { SaveBar } from "@/components/save-bar";
import type { Format } from "@/lib/format";
import type { TFor } from "@/lib/messages";
import { useFormat } from "@/lib/use-format";
import { useT } from "@/lib/use-t";

import { saveSearchSettings } from "./actions";
import type { Counted, SearchCatalog } from "./catalog";

type Settings = AdminSchemas["SearchSettingsResponse"];
type SettingsT = TFor<"settings">;

/**
 * A facet as saved: `option.<name>`, `field.<namespace>.<key>`, `vendor`,
 * `product_type` or `tag`, and its label.
 */
type Facet = { key: string; label: string };

/** Split listings by at most this many options (the API's limit). */
const MAX_SPLIT = 3;
/** Facets the API accepts. */
const MAX_FACETS = 20;

const OPTION = "option.";
/** A custom field's facet: `field.<namespace>.<key>`. */
const FIELD = "field.";

/** What a facet filters by, in the merchant's words. */
function kindOf(t: SettingsT, key: string): string {
  if (key.startsWith(OPTION)) return t("settings.search_filter_kind_option");
  return key.startsWith(FIELD)
    ? t("settings.search_filter_kind_custom_field")
    : t("settings.search_filter_kind_product_field");
}

/**
 * Everything the store could filter by, with the values found on its products: each
 * option name, then vendor, product type and tags. Saved facets the scan didn't see
 * (an option on older products) are kept, with no values to show.
 */
function candidates(t: SettingsT, catalog: SearchCatalog, saved: Facet[]) {
  const list = [
    ...catalog.options.map((o) => ({
      key: `${OPTION}${o.name}`,
      name: o.name,
      count: o.count,
      values: o.values,
    })),
    ...catalog.fields,
    {
      key: "vendor",
      name: t("settings.search_filter_vendor"),
      count: sum(catalog.vendors),
      values: catalog.vendors,
    },
    {
      key: "product_type",
      name: t("settings.search_filter_product_type"),
      count: sum(catalog.productTypes),
      values: catalog.productTypes,
    },
    {
      key: "tag",
      name: t("settings.search_filter_tags"),
      count: catalog.tags.length,
      values: catalog.tags,
    },
  ];
  for (const facet of saved) {
    if (!list.some((c) => same(c.key, facet.key))) {
      list.push({
        key: facet.key,
        name: facet.key.startsWith(OPTION)
          ? facet.key.slice(OPTION.length)
          : facet.key.startsWith(FIELD)
            ? facet.key.slice(FIELD.length)
            : facet.key,
        count: 0,
        values: [],
      });
    }
  }
  return list;
}

type Candidate = ReturnType<typeof candidates>[number];

const sum = (values: Counted[]) => values.reduce((n, v) => n + v.count, 0);
/** Option names match ignoring case, as search does. */
const same = (a: string, b: string) => a.toLowerCase() === b.toLowerCase();

/**
 * The filters storefronts show when the merchant picks none: product type, vendor,
 * the options most products have, tags.
 */
function defaults(all: Candidate[]): Facet[] {
  const options = all.filter((c) => c.key.startsWith(OPTION) && c.count > 0);
  return [
    { key: "product_type", label: "" },
    { key: "vendor", label: "" },
    ...options.map((o) => ({ key: o.key, label: "" })),
    { key: "tag", label: "" },
  ];
}

/**
 * Settings → Search: which filters shoppers get, which options split listings, with a
 * preview built from the store's own products. One draft, saved from the bar at the top.
 */
export function SearchSettingsForm({
  settings,
  catalog,
  status,
}: {
  settings: Settings;
  catalog: SearchCatalog;
  /** The index card, shown in the side column. */
  status: ReactNode;
}) {
  const t = useT<"settings">();
  const format = useFormat();
  const savedFacets: Facet[] = settings.facets.map((f) => ({ key: f.key, label: f.label ?? "" }));
  const all = candidates(t, catalog, savedFacets);
  const [custom, setCustom] = useState(savedFacets.length > 0);
  const [facets, setFacets] = useState<Facet[]>(savedFacets);
  const [split, setSplit] = useState<string[]>(settings.split_by);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();

  const request = () => ({
    split_by: split,
    facets: custom ? facets.map((f) => ({ key: f.key, label: f.label.trim() || null })) : [],
  });
  const saved = JSON.stringify({
    split_by: settings.split_by,
    facets: settings.facets.map((f) => ({ key: f.key, label: f.label ?? null })),
  });
  const isDirty = JSON.stringify(request()) !== saved;
  const splitChanged = JSON.stringify(split) !== JSON.stringify(settings.split_by);
  const shown = custom ? facets : defaults(all);

  const candidate = (key: string) => all.find((c) => same(c.key, key));
  const update = (key: string, label: string) =>
    setFacets((before) => before.map((f) => (f.key === key ? { ...f, label } : f)));
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
  const discard = () => {
    setCustom(savedFacets.length > 0);
    setFacets(savedFacets);
    setSplit(settings.split_by);
    setError(undefined);
  };

  const unused = all.filter((c) => !facets.some((f) => same(f.key, c.key)));
  const options = all.filter((c) => c.key.startsWith(OPTION));

  return (
    <>
      <SaveBar
        isDirty={isDirty}
        isPending={isPending}
        error={error}
        canSave={!custom || facets.length > 0}
        onSave={save}
        onDiscard={discard}
      />

      <Card>
        <VStack gap={3}>
          <Text type="display-3" as="h2">
            {t("settings.search_intro_title")}
          </Text>
          <Text color="secondary">{t("settings.search_intro_description")}</Text>
          <VStack gap={1}>
            <Text>
              {t.rich("settings.search_intro_filters", {
                b: (chunks, key) => <strong key={key}>{chunks}</strong>,
              })}
            </Text>
            <Text>
              {t.rich("settings.search_intro_listings", {
                b: (chunks, key) => <strong key={key}>{chunks}</strong>,
              })}
            </Text>
          </VStack>
          <Text type="supporting" color="secondary">
            {catalog.hasMore
              ? t("settings.search_scanned_newest", { count: catalog.scanned })
              : t("settings.search_scanned_all", { count: catalog.scanned })}
          </Text>
        </VStack>
      </Card>

      <div className="detail-columns">
        <div>
          <Card>
            <VStack gap={4}>
              <VStack gap={1}>
                <Text type="display-3" as="h2">
                  {t("settings.search_filters_title")}
                </Text>
                <Text type="supporting" color="secondary">
                  {t("settings.search_filters_description")}
                </Text>
              </VStack>
              <Switch
                label={t("settings.search_filters_custom")}
                description={
                  custom
                    ? t("settings.search_filters_custom_on")
                    : t("settings.search_filters_custom_off")
                }
                value={custom}
                onChange={(on) => {
                  setCustom(on);
                  if (on && facets.length === 0) setFacets(defaults(all));
                }}
              />

              {custom && (
                <VStack gap={0}>
                  {facets.length === 0 && (
                    <Text color="secondary">{t("settings.search_filters_empty")}</Text>
                  )}
                  {facets.map((f, i) => {
                    const c = candidate(f.key);
                    return (
                      <div
                        key={f.key}
                        className={i > 0 ? "facet-row facet-row-divided" : "facet-row"}
                      >
                        <VStack gap={0.5}>
                          <HStack gap={2} vAlign="center">
                            <Text weight="medium">{c?.name ?? f.key}</Text>
                            <Text type="supporting" color="secondary">
                              {kindOf(t, f.key)}
                            </Text>
                          </HStack>
                          <Text type="supporting" color="secondary">
                            {sample(t, format, c?.values ?? [])}
                          </Text>
                        </VStack>
                        <TextInput
                          label={t("settings.search_filter_label_for", { name: c?.name ?? f.key })}
                          isLabelHidden
                          placeholder={c?.name ?? t("settings.search_filter_label_placeholder")}
                          value={f.label}
                          onChange={(label) => update(f.key, label)}
                        />
                        <HStack gap={0}>
                          <IconButton
                            label={t("settings.search_filter_move_up")}
                            icon={<ArrowUpIcon width={16} />}
                            variant="ghost"
                            size="sm"
                            isDisabled={i === 0}
                            onClick={() => move(i, i - 1)}
                          />
                          <IconButton
                            label={t("settings.search_filter_move_down")}
                            icon={<ArrowDownIcon width={16} />}
                            variant="ghost"
                            size="sm"
                            isDisabled={i === facets.length - 1}
                            onClick={() => move(i, i + 1)}
                          />
                          <IconButton
                            label={t("settings.search_filter_remove", { name: c?.name ?? f.key })}
                            icon={<XMarkIcon width={16} />}
                            variant="ghost"
                            size="sm"
                            onClick={() =>
                              setFacets((before) => before.filter((x) => x.key !== f.key))
                            }
                          />
                        </HStack>
                      </div>
                    );
                  })}
                  {unused.length > 0 && facets.length < MAX_FACETS && (
                    <VStack gap={2}>
                      <Divider />
                      <Text type="supporting" color="secondary">
                        {t("settings.search_filter_add")}
                      </Text>
                      <HStack gap={2} wrap="wrap">
                        {unused.map((c) => (
                          <Button
                            key={c.key}
                            label={c.count > 0 ? `${c.name} · ${c.count}` : c.name}
                            icon={<PlusIcon width={14} />}
                            variant="secondary"
                            size="sm"
                            onClick={() =>
                              setFacets((before) => [...before, { key: c.key, label: "" }])
                            }
                          />
                        ))}
                      </HStack>
                      <Text type="supporting" color="secondary">
                        {t("settings.search_filter_add_hint")}
                      </Text>
                    </VStack>
                  )}
                </VStack>
              )}
            </VStack>
          </Card>

          <Card>
            <VStack gap={4}>
              <VStack gap={1}>
                <Text type="display-3" as="h2">
                  {t("settings.search_listings_title")}
                </Text>
                <Text type="supporting" color="secondary">
                  {t("settings.search_listings_description", { max: MAX_SPLIT })}
                </Text>
              </VStack>
              {options.length === 0 ? (
                <Text color="secondary">{t("settings.search_listings_no_options")}</Text>
              ) : (
                <VStack gap={3}>
                  {options.map((o) => {
                    const checked = split.some((s) => same(s, o.name));
                    return (
                      <CheckboxInput
                        key={o.key}
                        label={o.name}
                        description={splitExample(t, o.name, o.values)}
                        value={checked}
                        isDisabled={!checked && split.length >= MAX_SPLIT}
                        disabledMessage={t("settings.search_listings_limit", { max: MAX_SPLIT })}
                        onChange={(on) =>
                          setSplit((before) =>
                            on ? [...before, o.name] : before.filter((s) => !same(s, o.name)),
                          )
                        }
                      />
                    );
                  })}
                </VStack>
              )}
              {splitChanged && (
                <Text type="supporting" color="secondary">
                  {t("settings.search_listings_rebuild_hint")}
                </Text>
              )}
            </VStack>
          </Card>
        </div>

        <div>
          <Card>
            <VStack gap={3}>
              <VStack gap={1}>
                <Text type="display-3" as="h2">
                  {t("settings.search_preview_title")}
                </Text>
                <Text type="supporting" color="secondary">
                  {t("settings.search_preview_description")}
                </Text>
              </VStack>
              <PreviewGroup
                title={t("common.price")}
                values={[]}
                note={t("settings.search_preview_price_note")}
              />
              <PreviewGroup
                title={t("settings.search_preview_availability")}
                values={[]}
                note={t("settings.search_preview_availability_note")}
              />
              {shown.map((f) => {
                const c = candidate(f.key);
                return (
                  <PreviewGroup
                    key={f.key}
                    title={f.label.trim() || c?.name || f.key}
                    values={c?.values ?? []}
                  />
                );
              })}
              {split.length > 0 && (
                <Text type="supporting" color="secondary">
                  {t("settings.search_preview_results", { options: split.join(" × ") })}
                </Text>
              )}
            </VStack>
          </Card>
          {status}
        </div>
      </div>
    </>
  );
}

/** "Red, Blue, Green and 4 more", or a note when the scan found none. */
function sample(t: SettingsT, format: Format, values: Counted[]): string {
  if (values.length === 0) return t("settings.search_sample_none");
  const first = format.list(
    values.slice(0, 3).map((v) => v.value),
    "unit",
  );
  const more = values.length - 3;
  return more > 0 ? t("settings.search_sample_more", { values: first, count: more }) : first;
}

/** "A product in Red and Blue shows as 2 results." */
function splitExample(t: SettingsT, name: string, values: Counted[]): string {
  const two = values.slice(0, 2).map((v) => v.value);
  const option = name.toLowerCase();
  if (two.length < 2) return t("settings.search_split_example_one", { option });
  return t("settings.search_split_example_two", { first: two[0], second: two[1], option });
}

/** One group of the mock filter panel: its title and up to 4 values with counts. */
function PreviewGroup({
  title,
  values,
  note,
}: {
  title: string;
  values: Counted[];
  note?: string;
}) {
  const t = useT<"settings">();
  return (
    <VStack gap={1}>
      <Text weight="medium">{title}</Text>
      {note ? (
        <Text type="supporting" color="secondary">
          {note}
        </Text>
      ) : values.length === 0 ? (
        <Text type="supporting" color="secondary">
          {t("settings.search_preview_no_values")}
        </Text>
      ) : (
        <>
          {values.slice(0, 4).map((v) => (
            <HStack key={v.value} gap={2} vAlign="center">
              <span className="facet-preview-box" aria-hidden />
              <StackItem size="fill">
                <Text type="supporting">{v.value}</Text>
              </StackItem>
              <Badge label={String(v.count)} variant="neutral" />
            </HStack>
          ))}
          {values.length > 4 && (
            <Text type="supporting" color="secondary">
              {t("settings.search_preview_show_more", { count: values.length - 4 })}
            </Text>
          )}
        </>
      )}
    </VStack>
  );
}
