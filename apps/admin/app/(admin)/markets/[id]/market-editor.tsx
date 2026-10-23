"use client";

import { Card } from "@astryxdesign/core/Card";
import { VStack } from "@astryxdesign/core/Layout";
import { MetadataList, MetadataListItem } from "@astryxdesign/core/MetadataList";
import { Switch } from "@astryxdesign/core/Switch";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { type ReactNode, useState, useTransition } from "react";

import { CountryPicker } from "@/components/country-picker";
import { SaveBar } from "@/components/save-bar";
import type { Country } from "@/lib/regions";

import { setMarketCountries, updateMarket } from "../actions";

type Market = AdminSchemas["MarketResponse"];

/**
 * A market's page: what it sells (`main`: the catalog and its prices, which save as
 * they change) beside its settings, the name, tax rule and countries, which are one
 * draft saved by the bar at the top. The page keys it by the saved settings, so a save
 * starts a fresh draft from them.
 */
export function MarketEditor({
  market,
  countries,
  taken,
  main,
}: {
  market: Market;
  countries: Country[];
  /** Countries other markets have (`takenCountries`). */
  taken: Record<string, string>;
  main: ReactNode;
}) {
  const [name, setName] = useState(market.name);
  const [includeTax, setIncludeTax] = useState(market.prices_include_tax);
  const [chosen, setChosen] = useState<string[]>(market.countries);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();

  const nameChanged = name.trim() !== market.name;
  const taxChanged = includeTax !== market.prices_include_tax;
  const countriesChanged =
    chosen.length !== market.countries.length || chosen.some((c) => !market.countries.includes(c));
  const isDirty = nameChanged || taxChanged || countriesChanged;
  const canSave = name.trim() !== "" && chosen.length > 0;

  const discard = () => {
    setName(market.name);
    setIncludeTax(market.prices_include_tax);
    setChosen(market.countries);
    setError(undefined);
  };

  const save = () => {
    if (!isDirty || !canSave) return;
    startTransition(async () => {
      if (nameChanged || taxChanged) {
        const result = await updateMarket(market.id, {
          name: nameChanged ? name : undefined,
          pricesIncludeTax: taxChanged ? includeTax : undefined,
        });
        if (result.error) return setError(result.error);
      }
      if (countriesChanged) {
        const result = await setMarketCountries(market.id, market.countries, chosen);
        if (result.error) return setError(result.error);
      }
      setError(undefined);
    });
  };

  return (
    <>
      <SaveBar
        isDirty={isDirty}
        isPending={isPending}
        error={error}
        canSave={canSave}
        onSave={save}
        onDiscard={discard}
      />
      <div className="detail-columns">
        <div>{main}</div>
        <div>
          <Card>
            <VStack gap={4}>
              <Text type="display-3" as="h2">
                Details
              </Text>
              <TextInput
                label="Name"
                value={name}
                onChange={setName}
                onEnter={save}
                status={
                  name.trim() ? undefined : { type: "error", message: "A market needs a name." }
                }
              />
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
            </VStack>
          </Card>
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
              <CountryPicker
                countries={countries}
                taken={taken}
                value={chosen}
                onChange={setChosen}
                description="Pick or search; the quick picks add only countries no other market has."
                status={
                  chosen.length === 0
                    ? { type: "error", message: "A market needs at least one country." }
                    : undefined
                }
              />
            </VStack>
          </Card>
        </div>
      </div>
    </>
  );
}
