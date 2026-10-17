"use server";

import type { AdminSchemas, ApiError } from "@hypha/sdk/admin";
import { revalidatePath } from "next/cache";
import { redirect } from "next/navigation";

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
  /** The product as the last step stored it, after a save that succeeded. */
  product?: AdminSchemas["ProductResponse"];
};

/**
 * Saves the product editor's draft (design note 0021, section 8): the product's PATCH,
 * then its options, then its whole variant list (`PUT …/variants`), then its categories
 * (`PUT …/categories`, design note 0022), each only when its part changed. Values created by the options step are found by option name and label
 * for the variants that select them. Stops at the first failure: what came before it
 * stays saved.
 */
export async function saveProduct(id: string, plan: SavePlan): Promise<SaveResult> {
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
    if (plan.categories) {
      const { error } = await client.PUT("/admin/products/{id}/categories", {
        params: { path: { id } },
        body: { categories: plan.categories },
      });
      if (error) return { error: describe(error), options };
      // The product as it is now, its categories included.
      const { data } = await client.GET("/admin/products/{id}", { params: { path: { id } } });
      product = data;
    }
    return { product };
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
): Promise<{ items: AdminSchemas["AssetResponse"][]; next?: string } | { error: string }> {
  const client = await sessionClient();
  let next = cursor;
  for (let read = 0; read < 5; read++) {
    const { data, error } = await client.GET("/admin/assets", {
      params: { query: { cursor: next, limit: 60 } },
    });
    if (error) return { error: describe(error) };
    const items = data.items.filter((a) => a.kind !== "file");
    next = data.next_cursor ?? undefined;
    if (items.length > 0 || !next) return { items, next };
  }
  return { items: [], next };
}

/** The channels the product is sold in (design note 0020): `channels` becomes the whole set. */
export async function setProductChannels(id: string, channels: string[]): Promise<EditResult> {
  const client = await sessionClient();
  const { error } = await client.PUT("/admin/products/{id}/channels", {
    params: { path: { id } },
    body: { channels },
  });
  if (error) return failed(error);
  refresh(id);
  return {};
}
