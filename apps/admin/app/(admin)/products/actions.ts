"use server";

import type { AdminSchemas, ApiError } from "@hypha/sdk/admin";
import { revalidatePath } from "next/cache";
import { redirect } from "next/navigation";

import type { FieldsPatch } from "@/components/custom-fields/actions";
import { type FieldsResponse, jsonObject, problemsOf } from "@/lib/custom-fields";
import { parseMoney } from "@/lib/format";
import { describe, loadCurrencies, sessionClient } from "@/lib/hypha";

import type { SavePlan, SelectionRef } from "./[id]/draft";

export type FormState = { error?: string };

export async function createProduct(
  _previous: FormState,
  input: { title: string; handle: string; price: string; currency: AdminSchemas["Currency"] },
): Promise<FormState> {
  const amount = parseMoney(input.price, input.currency, await loadCurrencies());
  if (amount === null) return { error: "Enter the price as a number, like 18 or 18.50." };
  const client = await sessionClient();
  const { data, error } = await client.POST("/admin/products", {
    body: {
      title: input.title,
      handle: input.handle,
      price: { amount, currency: input.currency },
    },
  });
  if (error) return { error: describe(error) };
  revalidatePath("/products");
  redirect(`/products/${data.id}`);
}

export async function setProductStatus(
  id: string,
  status: AdminSchemas["ProductStatus"],
): Promise<FormState> {
  const client = await sessionClient();
  const { error } = await client.PATCH("/admin/products/{id}/status", {
    params: { path: { id } },
    body: { status },
  });
  if (error) return { error: describe(error) };
  revalidatePath(`/products/${id}`);
  revalidatePath("/products");
  return {};
}

/** Deletes the product (its media stay in the library) and goes back to the list. */
export async function deleteProduct(id: string): Promise<FormState> {
  const client = await sessionClient();
  const { error } = await client.DELETE("/admin/products/{id}", {
    params: { path: { id } },
  });
  if (error) return { error: describe(error) };
  revalidatePath("/products");
  redirect("/products");
}

/** The outcome of an edit: a message to show, and the API's code and details for the
 * cases a form handles itself (removing variants with an option value). */
export type EditResult = { error?: string; code?: string; details?: unknown };

function failed(error: ApiError): EditResult {
  return { error: describe(error), code: error.code, details: error.details };
}

/** Edits change the product page; titles, handles and media also show in the list. */
function refresh(id: string) {
  revalidatePath(`/products/${id}`);
  revalidatePath("/products");
}

/** A save's outcome: the first failure's message, and the options as stored when the
 * save got that far, so the editor can point its draft at them. */
export type SaveResult = {
  error?: string;
  options?: AdminSchemas["OptionResponse"][];
  /** The product as the last of its own steps (details, options, variants) stored it,
   * after a save that succeeded; without its categories and channels when the save
   * changed those (they're below). */
  product?: AdminSchemas["ProductResponse"];
  /** The product's categories as stored, when the save set them. */
  categories?: string[];
  /** The product's channels as stored, when the save set them. */
  channels?: string[];
  /** The product's custom fields as stored, when the save set them (design note 0033). */
  customFields?: FieldsResponse;
  /** A refused custom fields write's problems by field key. */
  fieldProblems?: Record<string, string>;
};

/**
 * Saves the product editor's draft (design note 0021, section 8): the product's PATCH,
 * then its options, then its whole variant list (`PUT …/variants`), then its categories
 * (`PUT …/categories`, design note 0022), then its channels (`PUT …/channels`, design
 * note 0020), each only when its part changed; categories and channels together, after
 * the rest; then its custom fields (`PATCH …/custom-fields`, design note 0033). Values created by the options step are found by option name and label for
 * the variants that select them. Stops at the first failure: what came before it stays
 * saved.
 */
export async function saveProduct(
  id: string,
  plan: SavePlan,
  customFields?: FieldsPatch,
): Promise<SaveResult> {
  const client = await sessionClient();
  let product: AdminSchemas["ProductResponse"] | undefined;
  try {
    if (plan.product) {
      const { data, error } = await client.PATCH("/admin/products/{id}", {
        params: { path: { id } },
        body: plan.product,
      });
      if (error) return { error: describe(error) };
      product = data;
    }
    let options: AdminSchemas["OptionResponse"][] | undefined;
    if (plan.options) {
      const { data, error } = await client.PUT("/admin/products/{id}/options", {
        params: { path: { id } },
        body: { options: plan.options.options, remove_variants: plan.options.removeVariants },
      });
      if (error) return { error: describe(error) };
      product = data;
      options = data.options;
    }
    if (plan.variants) {
      const variants = plan.variants.map((v) => ({
        ...v,
        selections: v.selections.map((s) => valueId(s, options)),
      }));
      const { data, error } = await client.PUT("/admin/products/{id}/variants", {
        params: { path: { id } },
        body: { variants },
      });
      if (error) return { error: describe(error), options };
      product = data;
    }
    // Categories and channels are memberships, apart from the product and each other:
    // both at once. The editor takes what they stored into the product it holds (the
    // page's refresh brings the same), so no read of the product follows.
    const [categories, channels] = await Promise.all([
      plan.categories
        ? client.PUT("/admin/products/{id}/categories", {
            params: { path: { id } },
            body: { categories: plan.categories },
          })
        : undefined,
      plan.channels
        ? client.PUT("/admin/products/{id}/channels", {
            params: { path: { id } },
            body: { channels: plan.channels },
          })
        : undefined,
    ]);
    const failure = categories?.error ?? channels?.error;
    if (failure) return { error: describe(failure), options };
    const stored = {
      product,
      categories: categories?.data?.categories,
      channels: channels?.data?.channels,
    };
    if (customFields) {
      const { data, error } = await client.PATCH("/admin/products/{id}/custom-fields", {
        params: { path: { id } },
        body: {
          custom_fields: jsonObject(customFields.custom_fields),
          if_versions: customFields.if_versions,
        },
      });
      if (error) {
        return {
          ...stored,
          error: describe(error),
          options,
          fieldProblems: problemsOf(error.code, error.details),
        };
      }
      return { ...stored, customFields: data };
    }
    return stored;
  } finally {
    refresh(id);
  }
}

/** The value a selection names: its ID, or (created by this save) the value with its
 * label in the stored option with its name. */
function valueId(selection: SelectionRef, options?: AdminSchemas["OptionResponse"][]): string {
  if (selection.value.id) return selection.value.id;
  const option = options?.find((o) =>
    selection.option.id ? o.id === selection.option.id : o.name === selection.option.name.trim(),
  );
  const value = option?.values.find((v) => v.label === selection.value.label.trim());
  // Not found only if the options step stored something else; the API then names it.
  return value?.id ?? selection.value.label;
}

/** Attaches, reorders and detaches in one: `assets` becomes the whole list. */
export async function setProductMedia(id: string, assets: string[]): Promise<EditResult> {
  const client = await sessionClient();
  const { error } = await client.PUT("/admin/products/{id}/media", {
    params: { path: { id } },
    body: { assets },
  });
  if (error) return failed(error);
  refresh(id);
  revalidatePath("/media");
  return {};
}

export async function setVariantMedia(
  id: string,
  variant: string,
  asset: string | null,
): Promise<EditResult> {
  const client = await sessionClient();
  const { error } = await client.PUT("/admin/products/{id}/variants/{variant_id}/media", {
    params: { path: { id, variant_id: variant } },
    body: { asset },
  });
  if (error) return failed(error);
  refresh(id);
  return {};
}

/**
 * A page of the library's images and videos, for picking media for a product. The
 * library lists one kind or all, so PDFs are dropped here; pages of only PDFs are read
 * past (a few at most), so a page is empty only when the library has nothing more.
 */
export async function libraryPage(
  cursor?: string,
  q?: string,
): Promise<{ items: AdminSchemas["AssetResponse"][]; next?: string } | { error: string }> {
  const client = await sessionClient();
  let next = cursor;
  for (let read = 0; read < 5; read++) {
    const { data, error } = await client.GET("/admin/assets", {
      params: { query: { cursor: next, limit: 60, q: q?.trim() || undefined } },
    });
    if (error) return { error: describe(error) };
    const items = data.items.filter((a) => a.kind !== "file");
    next = data.next_cursor ?? undefined;
    if (items.length > 0 || !next) return { items, next };
  }
  return { items: [], next };
}

/** A variant's stock (design note 0004): `id` is `null` while it isn't tracked. */
export type Stock = AdminSchemas["InventoryResponse"];

/** What the stock dialog saves: tracking, and the count on hand per location with the
 * count it showed, so a sale in the meantime isn't overwritten. */
export type StockChange = {
  tracked: boolean;
  continueSelling: boolean;
  levels: { location: string; onHand: number; expected: number | null }[];
};

export async function variantStock(variant: string): Promise<{ stock?: Stock; error?: string }> {
  const client = await sessionClient();
  const { data, error } = await client.GET("/admin/variants/{id}/inventory", {
    params: { path: { id: variant } },
  });
  return error ? { error: describe(error) } : { stock: data };
}

/**
 * Saves a variant's stock at once, not with the product's Save: starts tracking it when
 * it wasn't (creating its inventory item), changes tracking or overselling, then sets
 * each location's count that changed. Stops at the first failure.
 */
export async function saveVariantStock(
  productId: string,
  current: Stock,
  change: StockChange,
): Promise<{ stock?: Stock; error?: string }> {
  const client = await sessionClient();
  try {
    let stock = current;
    if (stock.id == null) {
      if (!change.tracked) return { stock };
      const { data, error } = await client.POST("/admin/inventory-items", {
        body: { variant: current.variant, tracked: true, continue_selling: change.continueSelling },
      });
      if (error) return { error: describe(error) };
      stock = data;
    } else if (
      stock.tracked !== change.tracked ||
      stock.continue_selling !== change.continueSelling
    ) {
      const { data, error } = await client.PATCH("/admin/inventory-items/{id}", {
        params: { path: { id: stock.id } },
        body: { tracked: change.tracked, continue_selling: change.continueSelling },
      });
      if (error) return { error: describe(error) };
      stock = data;
    }
    const item = stock.id;
    if (item == null || !change.tracked) return { stock };
    for (const level of change.levels) {
      if (level.onHand === level.expected) continue;
      const { data, error } = await client.PUT("/admin/inventory-items/{id}/levels/{location_id}", {
        params: { path: { id: item, location_id: level.location } },
        body: { on_hand: level.onHand, expected_on_hand: level.expected },
      });
      if (error) return { error: describe(error), stock };
      stock = data;
    }
    return { stock };
  } finally {
    refresh(productId);
  }
}
