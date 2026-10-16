"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { MultiSelector } from "@astryxdesign/core/MultiSelector";
import { Selector } from "@astryxdesign/core/Selector";
import { Switch } from "@astryxdesign/core/Switch";
import { TextInput } from "@astryxdesign/core/TextInput";
import { useState, useTransition } from "react";

import { useCurrencies } from "@/components/currencies";
import { currencyCodes } from "@/lib/format";
import { type Country, countryOptions, currencyOptions } from "@/lib/regions";

import { createMarket } from "../actions";

export function NewMarketForm({ countries }: { countries: Country[] }) {
  const currencies = useCurrencies();
  const [name, setName] = useState("");
  const [chosen, setChosen] = useState<string[]>([]);
  const [currency, setCurrency] = useState<string>();
  const [pricesIncludeTax, setPricesIncludeTax] = useState(false);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();

  const submit = () => {
    if (!name.trim() || chosen.length === 0 || !currency) return;
    startTransition(async () => {
      // Success redirects to the new market's page.
      const result = await createMarket({ name, countries: chosen, currency, pricesIncludeTax });
      if (result?.error) setError(result.error);
    });
  };

  return (
    <Card>
      <VStack gap={4}>
        {error && <Banner status="error" title={error} />}
        <TextInput
          label="Name"
          placeholder="Eurozone"
          description="For staff only; shoppers see their country."
          value={name}
          onChange={setName}
          onEnter={submit}
          hasAutoFocus
        />
        <MultiSelector
          label="Countries"
          description="A country belongs to one market at most."
          options={countryOptions(countries)}
          value={chosen}
          onChange={setChosen}
          hasSearch
          triggerDisplay="badges"
          maxBadges={6}
          placeholder="Choose countries"
        />
        <Selector
          label="Currency"
          description="What the market's prices are in. It can't change later: price lists depend on it."
          options={currencyOptions(currencyCodes(currencies))}
          value={currency}
          onChange={setCurrency}
          hasSearch
          placeholder="Choose a currency"
        />
        <Switch
          label="Prices include tax"
          description="On for most of Europe; off where tax is added at checkout, as in the US."
          value={pricesIncludeTax}
          onChange={setPricesIncludeTax}
        />
        <HStack gap={2} hAlign="end">
          <Button label="Cancel" variant="ghost" href="/markets" />
          <Button
            label="Add market"
            variant="primary"
            isLoading={isPending}
            isDisabled={!name.trim() || chosen.length === 0 || !currency}
            onClick={submit}
          />
        </HStack>
      </VStack>
    </Card>
  );
}
