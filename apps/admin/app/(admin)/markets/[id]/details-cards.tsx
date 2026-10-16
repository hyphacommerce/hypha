"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { MetadataList, MetadataListItem } from "@astryxdesign/core/MetadataList";
import { MultiSelector } from "@astryxdesign/core/MultiSelector";
import { Switch } from "@astryxdesign/core/Switch";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import type { AdminSchemas } from "@hypha/sdk/admin";
import { useState, useTransition } from "react";

import { type Country, countryOptions } from "@/lib/regions";

import { setMarketCountries, updateMarket } from "../actions";

type Market = AdminSchemas["MarketResponse"];

/** The market's name and tax rule, saved together; its currency is fixed. */
export function DetailsCard({ market }: { market: Market }) {
  const [name, setName] = useState(market.name);
  const [includeTax, setIncludeTax] = useState(market.prices_include_tax);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const isChanged = name.trim() !== market.name || includeTax !== market.prices_include_tax;

  const save = () => {
    if (!isChanged || !name.trim()) return;
    startTransition(async () => {
      const result = await updateMarket(market.id, {
        name: name.trim() !== market.name ? name : undefined,
        pricesIncludeTax: includeTax !== market.prices_include_tax ? includeTax : undefined,
      });
      setError(result.error);
    });
  };

  return (
    <Card>
      <VStack gap={4}>
        <Text type="display-3" as="h2">
          Details
        </Text>
        {error && <Banner status="error" title={error} />}
        <TextInput label="Name" value={name} onChange={setName} onEnter={save} />
        <Switch
          label="Prices include tax"
          description="Whether the prices shoppers in this market see already contain tax."
          value={includeTax}
          onChange={setIncludeTax}
        />
        <MetadataList>
          <MetadataListItem label="Currency">
            <VStack gap={0}>
              <Text>{market.currency}</Text>
              <Text type="supporting" color="secondary">
                Fixed once the market is created: its price lists are in it.
              </Text>
            </VStack>
          </MetadataListItem>
        </MetadataList>
        <HStack hAlign="end">
          <Button
            label="Save"
            variant="primary"
            isLoading={isPending}
            isDisabled={!isChanged || !name.trim()}
            onClick={save}
          />
        </HStack>
      </VStack>
    </Card>
  );
}

/** The countries the market sells to; Save adds and removes the difference. */
export function CountriesCard({ market, countries }: { market: Market; countries: Country[] }) {
  const [chosen, setChosen] = useState<string[]>(market.countries);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const isChanged =
    chosen.length !== market.countries.length || chosen.some((c) => !market.countries.includes(c));

  const save = () => {
    if (!isChanged || chosen.length === 0) return;
    startTransition(async () => {
      const result = await setMarketCountries(market.id, market.countries, chosen);
      setError(result.error);
    });
  };

  return (
    <Card>
      <VStack gap={4}>
        <VStack gap={1}>
          <Text type="display-3" as="h2">
            Countries
          </Text>
          <Text type="supporting" color="secondary">
            Shoppers in these countries buy from this market. A country belongs to one market.
          </Text>
        </VStack>
        {error && <Banner status="error" title={error} />}
        <MultiSelector
          label="Countries"
          isLabelHidden
          options={countryOptions(countries)}
          value={chosen}
          onChange={setChosen}
          hasSearch
          triggerDisplay="badges"
          maxBadges={8}
          status={
            chosen.length === 0
              ? { type: "error", message: "A market needs at least one country." }
              : undefined
          }
        />
        <HStack gap={2} hAlign="end">
          {isChanged && (
            <Button label="Discard" variant="ghost" onClick={() => setChosen(market.countries)} />
          )}
          <Button
            label="Save"
            variant="primary"
            isLoading={isPending}
            isDisabled={!isChanged || chosen.length === 0}
            onClick={save}
          />
        </HStack>
      </VStack>
    </Card>
  );
}
