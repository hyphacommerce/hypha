"use client";

import type { AdminSchemas } from "@hypha/sdk/admin";
import { type ReactNode, useEffect, useState, useTransition } from "react";

import { useCurrencies } from "@/components/currencies";
import { SaveBar } from "@/components/save-bar";
import type { CategoryNode } from "@/lib/categories";

import { type Stock, saveProduct } from "../actions";
import { CategoriesCard } from "./categories-card";
import { DetailsForm } from "./details-form";
import { type Draft, changes, fromProduct, plan, rebaseOptions } from "./draft";
import { OptionsEditor } from "./options-editor";
import { OrganisationCard } from "./organisation-card";
import { VariantsSection } from "./variants-section";

type Product = AdminSchemas["ProductResponse"];

/**
 * The product's details, content, categories, options and variants as one draft (design
 * notes 0021, section 8, and 0022, section 5): edits change only the draft, and a bar at
 * the top saves or discards them together. Media, publishing and stock act at once, as
 * before: an upload can't wait for Save, and publishing is its own decision. Two columns
 * (design note 0024, section 6): what the product is on the left, how it's sold and
 * organised on the right.
 *
 * When the saved product changes under the editor (after a save, an upload, publishing),
 * the draft follows it unless it has unsaved changes of its own.
 */
export function ProductEditor({
  product,
  summary,
  publishing,
  media,
  identifiers,
  categoryTree,
  stock,
  locations,
}: {
  product: Product;
  /** Cards for the side column, around the draft's own. */
  summary: ReactNode;
  publishing: ReactNode;
  media: ReactNode;
  identifiers: ReactNode;
  /** `undefined` when the tree couldn't be read: no Categories card then. */
  categoryTree?: CategoryNode[];
  /** The variants' stock by variant ID; `undefined` without `inventory:read`. */
  stock?: Record<string, Stock>;
  locations: AdminSchemas["LocationResponse"][];
}) {
  const currencies = useCurrencies();
  const currency = product.variants[0]?.price.currency ?? "EUR";
  const [base, setBase] = useState(product);
  const [draft, setDraft] = useState(() => fromProduct(product, currencies));
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();

  const saved = fromProduct(base, currencies);
  const changed = changes(draft, saved, currency, currencies);
  const isDirty = changed.product || changed.options || changed.variants || changed.categories;

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
      <SaveBar
        isDirty={isDirty}
        isPending={isPending}
        error={error}
        onSave={save}
        onDiscard={discard}
      />

      <div className="product-editor-columns">
        <div>
          <DetailsForm
            draft={draft}
            savedHandle={base.handle}
            isActive={base.status === "active"}
            onChange={edit}
          />
          {media}
          <OptionsEditor draft={draft} savedTitles={savedTitles} onChange={edit} />
          <VariantsSection
            product={base}
            draft={draft}
            currency={currency}
            stock={stock}
            locations={locations}
            onChange={edit}
          />
        </div>
        <div>
          {summary}
          {publishing}
          {categoryTree && <CategoriesCard tree={categoryTree} draft={draft} onChange={edit} />}
          <OrganisationCard draft={draft} onChange={edit} />
          {identifiers}
        </div>
      </div>
    </>
  );
}
