"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Selector } from "@astryxdesign/core/Selector";
import { TextInput } from "@astryxdesign/core/TextInput";
import { useState, useTransition } from "react";

import { useCurrencies } from "@/components/currencies";
import { currencyCodes } from "@/lib/format";
import { currencyOptions } from "@/lib/regions";
import { useFormat } from "@/lib/use-format";
import { useT } from "@/lib/use-t";

import { createSupplier } from "./actions";

/**
 * Adds a supplier from its name and currency, then opens its page for the rest
 * (contacts, terms, catalogue).
 */
export function NewSupplier({ currency: storeCurrency }: { currency: string | null }) {
  const t = useT<"suppliers">();
  const format = useFormat();
  const currencies = useCurrencies();
  const [isOpen, setIsOpen] = useState(false);
  const [name, setName] = useState("");
  const [currency, setCurrency] = useState(storeCurrency ?? "");
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const close = () => {
    setIsOpen(false);
    setName("");
    setCurrency(storeCurrency ?? "");
    setError(undefined);
  };
  const canSave = name.trim() !== "" && currency !== "";
  const save = () => {
    if (!canSave) return;
    startTransition(async () => {
      const result = await createSupplier({ name, currency });
      if (result?.error) setError(result.error);
    });
  };
  return (
    <>
      <Button
        label={t("suppliers.add_supplier")}
        variant="primary"
        onClick={() => setIsOpen(true)}
      />
      <Dialog isOpen={isOpen} onOpenChange={(open) => !open && close()} width={440} purpose="form">
        <DialogHeader
          title={t("suppliers.new_title")}
          subtitle={t("suppliers.new_subtitle")}
          onOpenChange={(open) => !open && close()}
        />
        <VStack gap={4} padding={4}>
          {error && <Banner status="error" title={error} />}
          <TextInput
            label={t("common.name")}
            value={name}
            onChange={setName}
            placeholder={t("suppliers.new_name_placeholder")}
          />
          <Selector
            label={t("common.currency")}
            description={t("suppliers.new_currency_description")}
            options={currencyOptions(currencyCodes(currencies), format.locale)}
            value={currency || undefined}
            onChange={setCurrency}
            placeholder={t("common.choose_currency")}
            hasSearch
          />
          <HStack gap={2} hAlign="end">
            <Button label={t("common.cancel")} variant="ghost" onClick={close} />
            <Button
              label={t("suppliers.new_submit")}
              variant="primary"
              isLoading={isPending}
              isDisabled={!canSave}
              onClick={save}
            />
          </HStack>
        </VStack>
      </Dialog>
    </>
  );
}
