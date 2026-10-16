"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { HStack } from "@astryxdesign/core/Layout";
import { Text } from "@astryxdesign/core/Text";
import type { AdminSchemas } from "@hypha/sdk/admin";
import { type ReactNode, useEffect, useState, useTransition } from "react";

import { useCurrencies } from "@/components/currencies";

import { saveProduct } from "../actions";
import { ContentForm } from "./content-form";
import { DetailsForm } from "./details-form";
import { type Draft, changes, fromProduct, plan, rebaseOptions } from "./draft";
import { OptionsEditor } from "./options-editor";
import { VariantsSection } from "./variants-section";

type Product = AdminSchemas["ProductResponse"];

/**
 * The product's details, content, options and variants as one draft (design note 0021,
 * section 8): edits change only the draft, and a bar at the top saves or discards them
 * together. Media and publishing (`media`, `publishing`) act at once, as before: an
 * upload can't wait for Save, and publishing is its own decision.
 *
 * When the saved product changes under the editor (after a save, an upload, publishing),
 * the draft follows it unless it has unsaved changes of its own.
 */
export function ProductEditor({
  product,
  publishing,
  media,
}: {
  product: Product;
  publishing: ReactNode;
  media: ReactNode;
}) {
  const currencies = useCurrencies();
  const currency = product.variants[0]?.price.currency ?? "EUR";
  const [base, setBase] = useState(product);
  const [draft, setDraft] = useState(() => fromProduct(product, currencies));
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();

  const saved = fromProduct(base, currencies);
  const changed = changes(draft, saved, currency, currencies);
  const isDirty = changed.product || changed.options || changed.variants;

  // A new saved product (React's "adjust state while rendering"): take it in, and reset
  // the draft to it unless the draft holds unsaved edits.
  const [seen, setSeen] = useState(product);
  if (product !== seen) {
    setSeen(product);
    setBase(product);
    if (!isDirty) setDraft(fromProduct(product, currencies));
  }

  useEffect(() => {
    if (!isDirty) return;
    const warn = (event: BeforeUnloadEvent) => event.preventDefault();
    window.addEventListener("beforeunload", warn);
    return () => window.removeEventListener("beforeunload", warn);
  }, [isDirty]);

  const edit = (change: Partial<Draft>) => {
    setDraft((before) => ({ ...before, ...change }));
    setError(undefined);
  };
  const discard = () => {
    setDraft(saved);
    setError(undefined);
  };
  const save = () => {
    const built = plan(draft, saved, currency, currencies);
    if ("error" in built) {
      setError(built.error);
      return;
    }
    startTransition(async () => {
      const result = await saveProduct(product.id, built.plan);
      if (result.error) {
        setError(result.error);
        // The options were stored before the failure: point the draft at them, so
        // saving again doesn't add their new values a second time.
        const stored = result.options;
        if (stored) setDraft((before) => rebaseOptions(before, stored));
        return;
      }
      setError(undefined);
      // What the API stored, at once; the page's refresh then brings the same product.
      if (result.product) {
        setBase(result.product);
        setDraft(fromProduct(result.product, currencies));
      }
    });
  };

  const savedTitles = Object.fromEntries(
    base.variants.map((v) => [v.id, v.title ?? "Default"]),
  );

  return (
    <>
      {(isDirty || error) && (
        <div
          style={{
            position: "sticky",
            top: 0,
            zIndex: 10,
            padding: "12px 16px",
            borderRadius: 8,
            background: "var(--color-background-muted)",
          }}
        >
          <HStack gap={3} hAlign="between" vAlign="center" wrap="wrap">
            <Text weight="medium">{isDirty ? "Unsaved changes" : "Not saved"}</Text>
            <HStack gap={2}>
              <Button label="Discard" variant="ghost" isDisabled={isPending} onClick={discard} />
              <Button
                label="Save"
                variant="primary"
                isLoading={isPending}
                isDisabled={!isDirty}
                onClick={save}
              />
            </HStack>
          </HStack>
          {error && (
            <div style={{ marginTop: 12 }}>
              <Banner status="error" title={error} />
            </div>
          )}
        </div>
      )}

      <DetailsForm
        draft={draft}
        savedHandle={base.handle}
        isActive={base.status === "active"}
        onChange={edit}
      />
      {publishing}
      <ContentForm draft={draft} onChange={edit} />
      {media}
      <OptionsEditor draft={draft} savedTitles={savedTitles} onChange={edit} />
      <VariantsSection product={base} draft={draft} currency={currency} onChange={edit} />
    </>
  );
}
