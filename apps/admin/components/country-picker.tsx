"use client";

// Picking a market's countries (design note 0024, section 6): a searchable list with
// quick picks for every free country or a region, and Clear. A country belongs to one
// market, so the ones other markets have are shown but can't be picked, and a quick
// pick takes only the free ones, saying how many it left out.

import { Button } from "@astryxdesign/core/Button";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { MultiSelector } from "@astryxdesign/core/MultiSelector";
import { Text } from "@astryxdesign/core/Text";
import { useState } from "react";

import { type Country, REGIONS, countryName, countryOptions } from "@/lib/regions";

export function CountryPicker({
  countries,
  taken,
  value,
  onChange,
  label = "Countries",
  description = "A country belongs to one market at most.",
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
  const [note, setNote] = useState<string>();
  const known = new Set(countries.map((c) => c.code));
  const free = (codes: string[]) => codes.filter((c) => known.has(c) && !taken[c]);

  const add = (codes: string[], what: string) => {
    const available = free(codes);
    const left = codes.filter((c) => known.has(c)).length - available.length;
    onChange([...new Set([...value, ...available])]);
    setNote(
      left > 0
        ? `Added ${what}; ${left} ${left === 1 ? "country is" : "countries are"} in other markets.`
        : undefined,
    );
  };

  const options = countryOptions(countries).map((option) => {
    const market = taken[option.value];
    return market ? { ...option, label: `${option.label} · in ${market}`, disabled: true } : option;
  });
  const names = value.map((code) => countryName(code, countries));

  return (
    <VStack gap={2}>
      <MultiSelector
        label={label}
        description={description}
        options={options}
        value={value}
        onChange={(codes) => {
          onChange(free(codes));
          setNote(undefined);
        }}
        hasSearch
        triggerDisplay="count"
        formatValue={(items) =>
          items.length === 1 ? (items[0]?.label ?? "1 country") : `${items.length} countries`
        }
        placeholder="Choose countries"
        status={status}
      />
      <HStack gap={1} wrap="wrap">
        <Button
          label={`All available (${free(countries.map((c) => c.code)).length})`}
          size="sm"
          variant="secondary"
          onClick={() =>
            add(
              countries.map((c) => c.code),
              "every free country",
            )
          }
        />
        {REGIONS.map((region) => (
          <Button
            key={region.key}
            label={region.label}
            size="sm"
            variant="ghost"
            onClick={() => add(region.codes, region.label)}
          />
        ))}
        {value.length > 0 && (
          <Button
            label="Clear"
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
            ? names.join(", ")
            : `${names.slice(0, 6).join(", ")} and ${names.length - 6} more`}
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
