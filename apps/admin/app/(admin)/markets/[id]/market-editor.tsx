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
import { useT } from "@/lib/use-t";

import { setMarketCountries, updateMarket } from "../actions";
import { LanguagesCard } from "./languages-card";

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
  languages,
  main,
}: {
  market: Market;
  countries: Country[];
  /** Countries other markets have (`takenCountries`). */
  taken: Record<string, string>;
  /**
   * The store's default language and what a market can offer (it and the published
   * ones); absent for a role that can't read the store, which then sees no card.
   */
  languages?: { storeDefault: string; offered: string[] };
  main: ReactNode;
}) {
  const t = useT<"markets">();
  const [name, setName] = useState(market.name);
  const [includeTax, setIncludeTax] = useState(market.prices_include_tax);
  const [chosen, setChosen] = useState<string[]>(market.countries);
  const [offers, setOffers] = useState<string[]>(market.languages);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();

  const nameChanged = name.trim() !== market.name;
  const taxChanged = includeTax !== market.prices_include_tax;
  const countriesChanged =
    chosen.length !== market.countries.length || chosen.some((c) => !market.countries.includes(c));
  // The order counts: the first language is the market's default.
  const languagesChanged =
    offers.length !== market.languages.length || offers.some((l, i) => l !== market.languages[i]);
  const isDirty = nameChanged || taxChanged || countriesChanged || languagesChanged;
  const canSave = name.trim() !== "" && chosen.length > 0;

  const discard = () => {
    setName(market.name);
    setIncludeTax(market.prices_include_tax);
    setChosen(market.countries);
    setOffers(market.languages);
    setError(undefined);
  };

  const save = () => {
    if (!isDirty || !canSave) return;
    startTransition(async () => {
      if (nameChanged || taxChanged || languagesChanged) {
        const result = await updateMarket(market.id, {
          name: nameChanged ? name : undefined,
          pricesIncludeTax: taxChanged ? includeTax : undefined,
          languages: languagesChanged ? offers : undefined,
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
                {t("common.details")}
              </Text>
              <TextInput
                label={t("common.name")}
                value={name}
                onChange={setName}
                onEnter={save}
                status={
                  name.trim()
                    ? undefined
                    : { type: "error", message: t("markets.detail_name_required") }
                }
              />
              <Switch
                label={t("markets.prices_include_tax")}
                description={t("markets.detail_prices_include_tax_description")}
                value={includeTax}
                onChange={setIncludeTax}
              />
              <MetadataList>
                <MetadataListItem label={t("common.currency")}>
                  <VStack gap={0}>
                    <Text>{market.currency}</Text>
                    <Text type="supporting" color="secondary">
                      {t("markets.detail_currency_fixed")}
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
                  {t("common.countries")}
                </Text>
                <Text type="supporting" color="secondary">
                  {t("markets.detail_countries_description")}
                </Text>
              </VStack>
              <CountryPicker
                countries={countries}
                taken={taken}
                value={chosen}
                onChange={setChosen}
                description={t("markets.detail_countries_picker_description")}
                status={
                  chosen.length === 0
                    ? { type: "error", message: t("markets.detail_countries_required") }
                    : undefined
                }
              />
            </VStack>
          </Card>
          {languages && (
            <LanguagesCard
              storeDefault={languages.storeDefault}
              offered={languages.offered}
              value={offers}
              onChange={setOffers}
            />
          )}
        </div>
      </div>
    </>
  );
}
