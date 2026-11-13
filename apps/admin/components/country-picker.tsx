"use client";

// Picking a market's countries: a searchable list with quick picks for every free
// country or a region, and Clear. A country belongs to one market, so the ones other
// markets have are shown but can't be picked, and a quick pick takes only the free
// ones, saying how many it left out.

import { Button } from "@astryxdesign/core/Button";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { MultiSelector } from "@astryxdesign/core/MultiSelector";
import { Text } from "@astryxdesign/core/Text";
import { useState } from "react";

import type { KeyIn } from "@/lib/messages";
import { type Country, REGIONS, countryName, countryOptions } from "@/lib/regions";
import { useFormat } from "@/lib/use-format";
import { useT } from "@/lib/use-t";

export function CountryPicker({
  countries,
  taken,
  value,
  onChange,
  label,
  description,
  status,
}: {
  countries: Country[];
  /** Countries other markets have, with the market's name (`takenCountries`). */
  taken: Record<string, string>;
  value: string[];
  onChange: (codes: string[]) => void;
  label?: string;
  description?: string;
  status?: { type: "error" | "warning" | "success"; message?: string };
}) {
  const t = useT();
  const format = useFormat();
  const [note, setNote] = useState<string>();
  const known = new Set(countries.map((c) => c.code));
  const free = (codes: string[]) => codes.filter((c) => known.has(c) && !taken[c]);

  /** Adds the free ones of `codes`: a region's (its label's key), or every country's. */
  const add = (codes: string[], region?: KeyIn<"components">) => {
    const available = free(codes);
    const left = codes.filter((c) => known.has(c)).length - available.length;
    onChange([...new Set([...value, ...available])]);
    setNote(
      left === 0
        ? undefined
        : region
          ? t("components.country_picker_added_region", { region: t(region), left })
          : t("components.country_picker_added_all", { left }),
    );
  };

  const options = countryOptions(countries, format.locale).map((option) => {
    const market = taken[option.value];
    return market
      ? {
          ...option,
          label: t("components.country_picker_taken", { country: option.label, market }),
          disabled: true,
        }
      : option;
  });
  const names = value.map((code) => countryName(code, countries, format.locale));

  return (
    <VStack gap={2}>
      <MultiSelector
        label={label ?? t("common.countries")}
        description={description ?? t("components.country_picker_description")}
        options={options}
        value={value}
        onChange={(codes) => {
          onChange(free(codes));
          setNote(undefined);
        }}
        hasSearch
        triggerDisplay="count"
        formatValue={(items) =>
          (items.length === 1 ? items[0]?.label : undefined) ??
          t("components.country_picker_count", { count: items.length })
        }
        placeholder={t("components.country_picker_placeholder")}
        status={status}
      />
      <HStack gap={1} wrap="wrap">
        <Button
          label={t("components.country_picker_all_available", {
            count: free(countries.map((c) => c.code)).length,
          })}
          size="sm"
          variant="secondary"
          onClick={() => add(countries.map((c) => c.code))}
        />
        {REGIONS.map((region) => (
          <Button
            key={region.key}
            label={t(region.label)}
            size="sm"
            variant="ghost"
            onClick={() => add(region.codes, region.label)}
          />
        ))}
        {value.length > 0 && (
          <Button
            label={t("common.clear")}
            size="sm"
            variant="ghost"
            onClick={() => {
              onChange([]);
              setNote(undefined);
            }}
          />
        )}
      </HStack>
      {value.length > 0 && (
        <Text type="supporting" color="secondary">
          {names.length <= 6
            ? format.list(names, "unit")
            : t("components.country_picker_names_more", {
                names: format.list(names.slice(0, 6), "unit"),
                count: names.length - 6,
              })}
        </Text>
      )}
      {note && (
        <Text type="supporting" color="secondary">
          {note}
        </Text>
      )}
    </VStack>
  );
}
