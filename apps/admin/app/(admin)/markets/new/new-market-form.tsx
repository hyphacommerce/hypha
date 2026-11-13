"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Selector } from "@astryxdesign/core/Selector";
import { Switch } from "@astryxdesign/core/Switch";
import { TextInput } from "@astryxdesign/core/TextInput";
import { useState, useTransition } from "react";

import { CountryPicker } from "@/components/country-picker";
import { useCurrencies } from "@/components/currencies";
import { currencyCodes } from "@/lib/format";
import { type Country, currencyOptions } from "@/lib/regions";
import { useFormat } from "@/lib/use-format";
import { useT } from "@/lib/use-t";

import { createMarket } from "../actions";

export function NewMarketForm({
  countries,
  taken,
}: {
  countries: Country[];
  /** Countries other markets have (`takenCountries`). */
  taken: Record<string, string>;
}) {
  const t = useT<"markets">();
  const format = useFormat();
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
          label={t("common.name")}
          placeholder={t("markets.new_name_placeholder")}
          description={t("markets.new_name_description")}
          value={name}
          onChange={setName}
          onEnter={submit}
          hasAutoFocus
        />
        <CountryPicker countries={countries} taken={taken} value={chosen} onChange={setChosen} />
        <Selector
          label={t("common.currency")}
          description={t("markets.new_currency_description")}
          options={currencyOptions(currencyCodes(currencies), format.locale)}
          value={currency}
          onChange={setCurrency}
          hasSearch
          placeholder={t("common.choose_currency")}
        />
        <Switch
          label={t("markets.prices_include_tax")}
          description={t("markets.new_prices_include_tax_description")}
          value={pricesIncludeTax}
          onChange={setPricesIncludeTax}
        />
        <HStack gap={2} hAlign="end">
          <Button label={t("common.cancel")} variant="ghost" href="/markets" />
          <Button
            label={t("markets.new_submit")}
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
