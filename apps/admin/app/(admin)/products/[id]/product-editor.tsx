"use client";

import type { AdminSchemas } from "@hypha/sdk/admin";
import { type ReactNode, useEffect, useState, useTransition } from "react";

import { useCurrencies } from "@/components/currencies";
import { useCustomFields } from "@/components/custom-fields/custom-fields-card";
import { CustomFieldsForm, CustomFieldsFrame } from "@/components/custom-fields/custom-fields-form";
import { SaveBar } from "@/components/save-bar";
import type { CategoryNode } from "@/lib/categories";
import { type Definition, type FieldsResponse, isWritable } from "@/lib/custom-fields";

import { type Stock, saveProduct } from "../actions";
import { CategoriesCard } from "./categories-card";
import { DetailsForm } from "./details-form";
import { type Draft, changes, fromProduct, plan, rebaseOptions } from "./draft";
import { OptionsEditor } from "./options-editor";
import { OrganisationCard } from "./organisation-card";
import { PublishingCard } from "./publishing-card";
import { VariantsSection } from "./variants-section";

type Product = AdminSchemas["ProductResponse"];

const NO_DEFINITIONS: Definition[] = [];

/** No values, for a product whose fields weren't read. */
function emptyFields(owner: string): FieldsResponse {
  return { owner, custom_fields: {}, versions: {} };
}

/**
 * The product's details, content, publishing, categories, options and variants as one draft:
 * edits change only the draft, and a bar at the top saves or discards them together.
 * Media and stock act at once, as before: an upload can't wait for Save. Two columns: what
 * the product is on the left, how it's sold and organised on the right.
 *
 * When the saved product changes under the editor (after a save, an upload, a status change),
 * the draft follows it unless it has unsaved changes of its own.
 */
export function ProductEditor({
  product,
  summary,
  channels,
  media,
  identifiers,
  categoryTree,
  stock,
  locations,
  customFields,
  variantFields,
}: {
  product: Product;
  /** Cards for the side column, around the draft's own. */
  summary: ReactNode;
  /** Every sales channel; `undefined` without `channels:read`: no Publishing card then. */
  channels?: AdminSchemas["ChannelResponse"][];
  media: ReactNode;
  identifiers: ReactNode;
  /** `undefined` when the tree couldn't be read: no Categories card then. */
  categoryTree?: CategoryNode[];
  /** The variants' stock by variant ID; `undefined` without `inventory:read`. */
  stock?: Record<string, Stock>;
  locations: AdminSchemas["LocationResponse"][];
  /**
   * The product's custom fields, saved with the bar; `undefined` without
   * `custom_fields:read` or with no product definitions: no card then.
   */
  customFields?: {
    definitions: Definition[];
    fields: FieldsResponse;
    scopes: readonly string[] | null;
  };
  /** The variants' definitions: each saved variant's fields open in a dialog. */
  variantFields?: { definitions: Definition[]; scopes: readonly string[] | null };
}) {
  const currencies = useCurrencies();
  const currency = product.variants[0]?.price.currency ?? "EUR";
  const fieldDefinitions = customFields?.definitions ?? NO_DEFINITIONS;
  // A stable stand-in: a new object each render would read as new values from the server.
  const [noFields] = useState(() => emptyFields(product.id));
  const fields = useCustomFields({
    ownerType: "product",
    ownerId: product.id,
    definitions: fieldDefinitions,
    fields: customFields?.fields ?? noFields,
    currency,
  });
  const [base, setBase] = useState(product);
  const [draft, setDraft] = useState(() => fromProduct(product, currencies));
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();

  const saved = fromProduct(base, currencies);
  const changed = changes(draft, saved, currency, currencies);
  const isDirty =
    changed.product ||
    changed.options ||
    changed.variants ||
    changed.categories ||
    changed.channels ||
    fields.isDirty;

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
    fields.discard();
    setError(undefined);
  };
  const save = () => {
    const built = plan(draft, saved, currency, currencies);
    if ("error" in built) {
      setError(built.error);
      return;
    }
    const fieldPlan = fields.prepare();
    if ("error" in fieldPlan) {
      setError(fieldPlan.error);
      return;
    }
    startTransition(async () => {
      const result = await saveProduct(product.id, built.plan, fieldPlan.patch ?? undefined);
      if (result.customFields) fields.accept(result.customFields);
      if (result.error) {
        setError(result.error);
        if (result.fieldProblems) fields.reject(result.fieldProblems);
        // The options were stored before the failure: point the draft at them, so
        // saving again doesn't add their new values a second time.
        const stored = result.options;
        if (stored) setDraft((before) => rebaseOptions(before, stored));
        return;
      }
      setError(undefined);
      // What the API stored, at once; the page's refresh then brings the same product.
      const { categories: storedCategories, channels: storedChannels } = result;
      if (result.product || storedCategories || storedChannels) {
        const stored: Product = {
          ...(result.product ?? base),
          ...(storedCategories && { categories: storedCategories }),
          ...(storedChannels && { channels: storedChannels }),
        };
        setBase(stored);
        setDraft(fromProduct(stored, currencies));
      }
    });
  };

  const savedTitles = Object.fromEntries(base.variants.map((v) => [v.id, v.title ?? "Default"]));

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
            variantFields={variantFields}
            onChange={edit}
          />
        </div>
        <div>
          {summary}
          {channels && (
            <PublishingCard
              channels={channels}
              draft={draft}
              isActive={base.status === "active"}
              onChange={edit}
            />
          )}
          {categoryTree && <CategoriesCard tree={categoryTree} draft={draft} onChange={edit} />}
          <OrganisationCard draft={draft} onChange={edit} />
          {customFields && fieldDefinitions.length > 0 && (
            <CustomFieldsFrame ownerType="product">
              <CustomFieldsForm
                definitions={fieldDefinitions}
                inputs={fields.inputs}
                onChange={(key, input) => {
                  fields.edit(key, input);
                  setError(undefined);
                }}
                problems={fields.problems}
                isWritable={(d) => isWritable(d, "product", customFields.scopes)}
                redacted={fields.saved.redacted ?? undefined}
              />
            </CustomFieldsFrame>
          )}
          {identifiers}
        </div>
      </div>
    </>
  );
}
