"use server";

// Discounts: creating and changing them (the form builds the request, `lib/discounts/
// wire.ts`), the store's own summary of a draft, deleting them, and their codes.

import type { AdminSchemas, ApiError } from "@hyphacommerce/sdk/admin";
import { revalidatePath } from "next/cache";
import { redirect } from "next/navigation";

import type { CreateDiscount, Refusal, UpdateDiscount } from "@/lib/discounts/wire";
import { describe, sessionClient } from "@/lib/hypha";

/** The outcome of a write: why it was refused, if it was. */
export type Result = { refusal?: Refusal };

function refusal(error: ApiError): Refusal {
  return { code: error.code, message: describe(error), details: error.details };
}

/** The list and the discount's own pages, and orders (which show discounts' titles). */
function refresh() {
  revalidatePath("/discounts", "layout");
}

/** Creates the discount, with its first code, then opens it. */
export async function createDiscount(body: CreateDiscount): Promise<Result> {
  const client = await sessionClient();
  const { data, error } = await client.POST("/admin/discounts", { body });
  if (error) return { refusal: refusal(error) };
  refresh();
  redirect(`/discounts/${data.id}`);
}

/** Saves what changed (a JSON Merge Patch). */
export async function updateDiscount(id: string, body: UpdateDiscount): Promise<Result> {
  if (Object.keys(body).length === 0) return {};
  const client = await sessionClient();
  const { error } = await client.PATCH("/admin/discounts/{id}", {
    params: { path: { id } },
    body,
  });
  if (error) return { refusal: refusal(error) };
  refresh();
  return {};
}

/** The store's own summary of a discount, and the rules it breaks. */
export type SummaryAnswer = AdminSchemas["DiscountSummaryResponse"];

/**
 * The store's own summary of the discount a create would make, or a saved one with the
 * changes a `PATCH` would make (`null` while it breaks a rule, the `problems` saying
 * which); null when the request failed. It writes nothing: `discounts:read` is enough.
 */
export async function discountSummary(
  write: { id: null; body: CreateDiscount } | { id: string; body: UpdateDiscount },
): Promise<SummaryAnswer | null> {
  const client = await sessionClient();
  const body = write.id === null ? write.body : { ...write.body, id: write.id };
  const { data } = await client.POST("/admin/discounts/summary", { body });
  return data ?? null;
}

export async function deleteDiscount(id: string): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.DELETE("/admin/discounts/{id}", { params: { path: { id } } });
  if (error) return { refusal: refusal(error) };
  refresh();
  redirect("/discounts");
}

// ---- codes ----

/** The outcome of a codes write: a message, if it failed. */
export type CodesResult = { error?: string; taken?: string };

export async function addCodes(
  id: string,
  codes: { code: string; usage_limit: number | null }[],
  batch: string | null,
): Promise<CodesResult> {
  const client = await sessionClient();
  const { error } = await client.POST("/admin/discounts/{id}/codes", {
    params: { path: { id } },
    body: { codes, batch },
  });
  if (error) {
    const taken =
      error.code === "discount_code_taken" &&
      typeof error.details === "object" &&
      error.details !== null
        ? Reflect.get(error.details, "code")
        : undefined;
    return {
      error: typeof taken === "string" ? `Another discount has ${taken}.` : describe(error),
    };
  }
  refresh();
  return {};
}

export async function generateCodes(
  id: string,
  input: {
    count: number;
    prefix: string | null;
    length: number;
    usage_limit: number | null;
    batch: string | null;
  },
): Promise<CodesResult> {
  const client = await sessionClient();
  const { error } = await client.POST("/admin/discounts/{id}/codes/generate", {
    params: { path: { id } },
    body: input,
  });
  if (error) return { error: describe(error) };
  refresh();
  return {};
}

/** Removes the codes named, or a whole batch. */
export async function removeCodes(
  id: string,
  which: { codes: string[] } | { batch: string },
): Promise<CodesResult & { removed?: number }> {
  const client = await sessionClient();
  const { data, error } = await client.DELETE("/admin/discounts/{id}/codes", {
    params: { path: { id } },
    body: which,
  });
  if (error) return { error: describe(error) };
  refresh();
  return { removed: data.removed };
}

/**
 * Deactivates (`active: false`) or reactivates the codes named, or a whole batch. Carts
 * refuse an inactive code as unknown; its uses so far stay.
 */
export async function setCodesActive(
  id: string,
  which: { codes: string[] } | { batch: string },
  active: boolean,
): Promise<CodesResult & { updated?: number }> {
  const client = await sessionClient();
  const { data, error } = await client.PATCH("/admin/discounts/{id}/codes", {
    params: { path: { id } },
    body: { ...which, active },
  });
  if (error) return { error: describe(error) };
  refresh();
  return { updated: data.updated };
}
