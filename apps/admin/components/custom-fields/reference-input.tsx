"use client";

// A reference field's input: the target's prefixed ID, picked where the admin has a
// picker (products from the product picker, categories from the tree) or typed, and
// named under the input once looked up.

import { Button } from "@astryxdesign/core/Button";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Selector } from "@astryxdesign/core/Selector";
import { TextInput } from "@astryxdesign/core/TextInput";
import { useEffect, useState, useTransition } from "react";

import { ProductPicker } from "@/components/product-picker";

import { categoryOptions, lookupReference } from "./actions";

const PREFIX: Record<string, string> = {
  product: "prod_",
  variant: "var_",
  category: "pcat_",
  customer: "cus_",
  asset: "asset_",
};

/** The categories, read once per page when a category reference first needs them. */
function useCategories(isNeeded: boolean) {
  const [options, setOptions] = useState<{ value: string; label: string }[]>();
  const [, startLoading] = useTransition();
  useEffect(() => {
    if (!isNeeded) return;
    startLoading(async () => setOptions(await categoryOptions()));
  }, [isNeeded]);
  return options;
}

export function ReferenceInput({
  label,
  description,
  kind,
  value,
  onChange,
  isDisabled,
  error,
}: {
  label: string;
  description?: string;
  kind: string;
  value: string;
  onChange: (value: string) => void;
  isDisabled?: boolean;
  error?: string;
}) {
  const [isPicking, setIsPicking] = useState(false);
  const [named, setNamed] = useState<{ id: string; label?: string; error?: string }>();
  const [isLooking, startLooking] = useTransition();
  const categories = useCategories(kind === "category");
  const status = error ? { type: "error" as const, message: error } : undefined;

  const id = value.trim();
  const lookUp = () =>
    startLooking(async () => {
      const found = await lookupReference(kind, id);
      setNamed({ id, ...found });
    });

  if (kind === "category" && categories) {
    return (
      <Selector
        label={label}
        description={description}
        isOptional
        placeholder="No category"
        options={categories}
        value={value || null}
        hasClear
        hasSearch
        isDisabled={isDisabled}
        status={status}
        onChange={(picked: string | null) => onChange(picked ?? "")}
        width="100%"
      />
    );
  }

  const shown = named?.id === id ? (named.label ?? named.error) : undefined;
  return (
    <VStack gap={1}>
      <HStack gap={2} vAlign="end">
        <TextInput
          label={label}
          description={shown ?? description}
          isOptional
          placeholder={`${PREFIX[kind] ?? ""}…`}
          value={value}
          onChange={onChange}
          isDisabled={isDisabled}
          status={status}
          onEnter={id ? lookUp : undefined}
        />
        {kind === "product" && !isDisabled && (
          <Button label="Pick" variant="secondary" onClick={() => setIsPicking(true)} />
        )}
        {id !== "" && (
          <Button label="Look up" variant="ghost" isLoading={isLooking} onClick={lookUp} />
        )}
      </HStack>
      {isPicking && (
        <ProductPicker
          title={`Pick a product for ${label}`}
          mode="single"
          confirmLabel={() => "Use this product"}
          onConfirm={async ([product]) => {
            if (product) {
              onChange(product.id);
              setNamed({ id: product.id, label: product.title });
            }
            setIsPicking(false);
          }}
          onClose={() => setIsPicking(false)}
        />
      )}
    </VStack>
  );
}

/** Adds references to a list field from a picker, where the admin has one. */
export function ReferenceListPicker({
  kind,
  onPick,
}: {
  kind: string;
  onPick: (ids: string[]) => void;
}) {
  const [isPicking, setIsPicking] = useState(false);
  const categories = useCategories(kind === "category");
  if (kind === "category") {
    return categories ? (
      <Selector
        label="Add a category"
        isLabelHidden
        placeholder="Add a category"
        options={categories}
        hasSearch
        size="sm"
        onChange={(picked: string) => onPick([picked])}
      />
    ) : null;
  }
  if (kind !== "product") return null;
  return (
    <>
      <HStack>
        <Button
          label="Pick products"
          size="sm"
          variant="secondary"
          onClick={() => setIsPicking(true)}
        />
      </HStack>
      {isPicking && (
        <ProductPicker
          title="Pick products"
          onConfirm={async (products) => {
            onPick(products.map((p) => p.id));
            setIsPicking(false);
          }}
          onClose={() => setIsPicking(false)}
        />
      )}
    </>
  );
}
