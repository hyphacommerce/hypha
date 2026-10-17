"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Selector } from "@astryxdesign/core/Selector";
import { TextInput } from "@astryxdesign/core/TextInput";
import type { AdminSchemas } from "@hypha/sdk/admin";
import { startTransition, useActionState, useState } from "react";

import { useCurrencies } from "@/components/currencies";
import { currencyCodes } from "@/lib/format";

import { createProduct } from "../actions";

type Currency = AdminSchemas["Currency"];

/** A handle from a title, by the API's rule: lowercase letters, digits, single dashes. */
function toHandle(title: string): string {
  return title
    .normalize("NFKD")
    .replace(/[̀-ͯ]/g, "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

export function NewProductForm() {
  const currencies = useCurrencies();
  const [title, setTitle] = useState("");
  // Follows the title until edited by hand; clearing it follows the title again.
  const [handle, setHandle] = useState<string>();
  const [price, setPrice] = useState("");
  const [currency, setCurrency] = useState<Currency>("EUR");
  const [state, create, isPending] = useActionState(createProduct, {});

  const effectiveHandle = handle ?? toHandle(title);
  const canSubmit = title.trim() !== "" && effectiveHandle !== "" && price.trim() !== "";
  const submit = () => {
    if (!canSubmit) return;
    startTransition(() => create({ title, handle: effectiveHandle, price, currency }));
  };

  return (
    <Card>
      <VStack gap={4}>
        {state.error && <Banner status="error" title={state.error} />}
        <TextInput
          label="Title"
          placeholder="Ceramic mug"
          value={title}
          onChange={setTitle}
          onEnter={submit}
          hasAutoFocus
        />
        <TextInput
          label="Handle"
          description="The product's address in the storefront: lowercase letters, digits and dashes."
          value={effectiveHandle}
          onChange={(value) => setHandle(value === "" ? undefined : value)}
          onEnter={submit}
        />
        <div className="form-grid">
          <TextInput
            label="Price"
            placeholder="18.00"
            value={price}
            onChange={setPrice}
            onEnter={submit}
          />
          <Selector
            label="Currency"
            options={currencyCodes(currencies)}
            hasSearch
            value={currency}
            onChange={(value) => setCurrency(value as Currency)}
          />
        </div>
        <HStack gap={2} hAlign="end">
          <Button label="Cancel" variant="ghost" href="/products" />
          <Button
            label="Save draft"
            variant="primary"
            isLoading={isPending}
            isDisabled={!canSubmit}
            onClick={submit}
          />
        </HStack>
      </VStack>
    </Card>
  );
}
