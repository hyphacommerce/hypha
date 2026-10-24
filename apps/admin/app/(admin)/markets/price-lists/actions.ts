"use server";

// Price lists: their settings, the markets they're on, and their fixed prices (the
// grid's saves and CSV imports).

import type { ApiError } from "@hyphacommerce/sdk/admin";
import { revalidatePath } from "next/cache";
import { redirect } from "next/navigation";

import { describe, loadCurrencies, loadTimeZone, sessionClient } from "@/lib/hypha";
import {
  type ImportPreview,
  type ImportRow,
  MAX_PRICES_PER_REQUEST,
  importVariantIds,
  previewPriceImport as previewCsv,
} from "@/lib/price-csv";
import type { PriceGridDiff } from "@/lib/price-grid/types";
import { type SaveResult, writeDiff } from "@/lib/price-grid/write";
import {
  type ImportFailure,
  type ImportStep,
  MAX_IMPORT_BYTES,
  importSteps,
  stepDiff,
  stepFailures,
} from "@/lib/price-import";
import {
  type FormErrors,
  type ListForm,
  type PriceList,
  catalogsInUse,
  createRequest,
  updateRequest,
} from "@/lib/price-lists";
import type { SignedUpload } from "@/lib/storage-upload";

import { allGridProducts, gridProductsByIds } from "./grid-data";

export type { SaveResult };

type Client = Awaited<ReturnType<typeof sessionClient>>;

/** The outcome of an edit: a message to show, if it failed. */
export type Result = { error?: string };

/** The lists' pages, the markets' (their cards) and the products' (their prices). */
function refresh() {
  revalidatePath("/markets", "layout");
  revalidatePath("/products", "layout");
}

/** A refused create or edit: on the field it's about, when it's about one. */
function refusedSettings(error: { code: string; error: string }): Result & {
  fieldErrors?: FormErrors;
} {
  switch (error.code) {
    case "price_list_name_taken":
      return { fieldErrors: { name: describe(error) } };
    case "ends_not_after_start":
      return { fieldErrors: { endsAt: describe(error) } };
    case "customer_group_not_found":
    case "too_many_audience_groups":
    case "duplicate_audience_group":
      return { fieldErrors: { groups: describe(error) } };
    default:
      return { error: describe(error) };
  }
}

/**
 * Creates the list and puts it on `catalogs` (the markets chosen on the form), then
 * opens it. A catalog that refuses it leaves the list created, and its page says so.
 */
export async function createPriceList(
  form: ListForm,
  catalogs: string[],
): Promise<Result & { fieldErrors?: FormErrors }> {
  const request = createRequest(form, await loadTimeZone());
  if ("errors" in request) return { fieldErrors: request.errors };
  const client = await sessionClient();
  const { data: list, error } = await client.POST("/admin/price-lists", { body: request.body });
  if (error) {
    return refusedSettings(error);
  }
  const attached = await Promise.all(
    catalogs.map((catalog) =>
      client.POST("/admin/catalogs/{id}/price-lists", {
        params: { path: { id: catalog } },
        body: { price_list: list.id },
      }),
    ),
  );
  refresh();
  // The page says so with a message of its own: a message in the URL could be anyone's.
  redirect(
    attached.some((r) => r.error)
      ? `/markets/price-lists/${list.id}?attach=failed`
      : `/markets/price-lists/${list.id}`,
  );
}

export async function updatePriceList(
  id: string,
  form: ListForm,
): Promise<Result & { fieldErrors?: FormErrors }> {
  const client = await sessionClient();
  const { data: list, error: readError } = await client.GET("/admin/price-lists/{id}", {
    params: { path: { id } },
  });
  if (!list) return { error: readError ? describe(readError) : "The list couldn't be read." };
  const request = updateRequest(form, list, await loadTimeZone());
  if ("errors" in request) return { fieldErrors: request.errors };
  if (Object.keys(request.body).length === 0) return {};
  const { error } = await client.PATCH("/admin/price-lists/{id}", {
    params: { path: { id } },
    body: request.body,
  });
  if (error) {
    return refusedSettings(error);
  }
  refresh();
  return {};
}

/**
 * Deletes the list. While markets use it the API refuses: then the answer names them,
 * so the page can say which to take it off.
 */
export async function deletePriceList(id: string): Promise<Result & { markets?: string[] }> {
  const client = await sessionClient();
  const { error } = await client.DELETE("/admin/price-lists/{id}", { params: { path: { id } } });
  if (error) {
    if (error.code !== "price_list_in_use") return { error: describe(error) };
    const catalogs = catalogsInUse(error.details);
    const markets = await client.GET("/admin/markets");
    const names = catalogs.map(
      (catalog) => markets.data?.items.find((m) => m.catalog === catalog)?.name ?? catalog,
    );
    return { error: describe(error), markets: names };
  }
  refresh();
  redirect("/markets/price-lists");
}

/** Puts the list on a market's catalog, beside the lists already there. */
export async function addToCatalog(list: string, catalog: string): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.POST("/admin/catalogs/{id}/price-lists", {
    params: { path: { id: catalog } },
    body: { price_list: list },
  });
  if (error) return { error: describe(error) };
  refresh();
  return {};
}

/** Takes the list off a market's catalog; the list stays. */
export async function removeFromCatalog(list: string, catalog: string): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.DELETE("/admin/catalogs/{id}/price-lists/{price_list}", {
    params: { path: { id: catalog, price_list: list } },
  });
  if (error) return { error: describe(error) };
  refresh();
  return {};
}

/**
 * Writes `diff` to the list's fixed prices: the grid's Save, and each step of applying
 * an import (at most 1,000 to set and 1,000 to remove, so its dialog shows progress).
 */
export async function saveFixedPrices(list: string, diff: PriceGridDiff): Promise<SaveResult> {
  const client = await sessionClient();
  const params = { path: { id: list } };
  const result = await writeDiff(diff, {
    put: (entries) =>
      client.PUT("/admin/price-lists/{id}/fixed-prices", {
        params,
        body: {
          fixed_prices: entries.map((e) => ({
            variant: e.variant,
            price: e.price,
            compare_at: e.compare_at,
            breaks: e.breaks,
          })),
        },
      }),
    remove: (variants) =>
      client.DELETE("/admin/price-lists/{id}/fixed-prices", { params, body: { variants } }),
    describe,
  });
  refresh();
  return result;
}

/**
 * Changes per step of applying. A file naming every row's variant ID reads only the
 * step's variants, so its steps are one `PUT` each; any other file is matched against
 * every product again at each step, so its steps are fewer and carry several `PUT`s.
 */
const STEP_BY_IDS = MAX_PRICES_PER_REQUEST;
const STEP_BY_CATALOG = 5 * MAX_PRICES_PER_REQUEST;

const TOO_LARGE = `That file is over ${MAX_IMPORT_BYTES / 1024 / 1024} MB. Split it into smaller files.`;

/** An upload's refusals, said for an import. */
function uploadMessage(error: ApiError): string {
  switch (error.code) {
    case "media_not_configured":
      return "Imports go through media storage, and this store has none set up. Configure media storage, then import again.";
    case "file_too_large":
      return TOO_LARGE;
    case "upload_missing":
      return "The upload didn't finish. Try again.";
    case "upload_not_found":
      return "The uploaded file has expired. Import it again.";
    default:
      return describe(error);
  }
}

/**
 * Starts an import: the upload that the browser sends the file to, straight to storage.
 * Nothing about the file passes through here but its name and size.
 */
export async function startPriceImport(file: {
  filename: string;
  byte_size: number;
}): Promise<{ id: string; upload: SignedUpload } | { error: string }> {
  if (file.byte_size > MAX_IMPORT_BYTES) return { error: TOO_LARGE };
  const client = await sessionClient();
  const { data, error } = await client.POST("/admin/uploads", {
    body: { purpose: "price_import", filename: file.filename, byte_size: file.byte_size },
  });
  if (error) return { error: uploadMessage(error) };
  return { id: data.id, upload: data.upload };
}

/** Deletes an import's upload: once applied, or when the dialog closes without. */
export async function discardPriceImport(upload: string): Promise<void> {
  const client = await sessionClient();
  // An upload left behind expires by itself: nothing to tell the person.
  await client.DELETE("/admin/uploads/{id}", { params: { path: { id: upload } } });
}

/** The list and the uploaded file's text, or why either can't be read. */
async function readImport(
  client: Client,
  list: string,
  upload: string,
): Promise<{ list: PriceList; text: string } | { error: string }> {
  const [read, file] = await Promise.all([
    client.GET("/admin/price-lists/{id}", { params: { path: { id: list } } }),
    client.GET("/admin/uploads/{id}/content", {
      params: { path: { id: upload } },
      parseAs: "text",
    }),
  ]);
  if (!read.data)
    return { error: read.error ? describe(read.error) : "The list couldn't be read." };
  if (file.data === undefined) {
    return { error: file.error ? uploadMessage(file.error) : "The file couldn't be read." };
  }
  return { list: read.data, text: file.data };
}

/**
 * What importing `text` (a CSV file's contents) into `list` would do: nothing is
 * written. A file naming every row's variant ID is matched against those variants only
 * (those on `lines`, with them), else against every product.
 */
async function matchImport(
  client: Client,
  list: PriceList,
  text: string,
  lines?: { from: number; to: number },
): Promise<{ preview: ImportPreview; byIds: boolean }> {
  const [categories, currencies] = await Promise.all([
    client.GET("/admin/categories"),
    loadCurrencies(),
  ]);
  const tree = categories.data?.items ?? [];
  const ids = importVariantIds(text, lines);
  const products = ids
    ? await gridProductsByIds(client, list, tree, ids)
    : await allGridProducts(client, list, tree, { fixedOnly: false });
  return { preview: previewCsv(text, products, list.currency, currencies), byIds: ids !== null };
}

/** The most rows an import's preview lists; the counts cover them all. */
const PREVIEW_ROWS = 500;

export type ImportPreviewResult = Omit<ImportPreview, "rows" | "diff"> & {
  /** The rows that do something or can't (unchanged ones left out), up to 500. */
  rows: ImportRow[];
  /** How many such rows were left out of `rows`. */
  more: number;
  /** Fixed prices applying sets and removes. */
  upserts: number;
  clears: number;
  /** What `applyPriceImport` is asked for, one after another. */
  steps: ImportStep[];
};

const emptyPreview = (error: string): ImportPreviewResult => ({
  rows: [],
  more: 0,
  counts: { changed: 0, unchanged: 0, unmatched: 0, invalid: 0 },
  upserts: 0,
  clears: 0,
  steps: [],
  error,
});

/** The import dialog's preview of the uploaded file: what it'd do, nothing written. */
export async function previewPriceImport(input: {
  list: string;
  upload: string;
}): Promise<ImportPreviewResult> {
  const client = await sessionClient();
  const read = await readImport(client, input.list, input.upload);
  if ("error" in read) return emptyPreview(read.error);
  const { preview, byIds } = await matchImport(client, read.list, read.text);
  if (preview.error) return emptyPreview(preview.error);
  const notable = preview.rows.filter((r) => r.status !== "unchanged");
  return {
    counts: preview.counts,
    rows: notable.slice(0, PREVIEW_ROWS),
    more: Math.max(0, notable.length - PREVIEW_ROWS),
    upserts: preview.diff.upserts.length,
    clears: preview.diff.clears.length,
    steps: importSteps(preview.rows, byIds ? STEP_BY_IDS : STEP_BY_CATALOG),
  };
}

export type ImportStepResult = {
  /** The rows the API refused. */
  failed: ImportFailure[];
  /** Why the step couldn't run at all: then nothing of it was written. */
  error?: string;
};

/**
 * Applies one step of an import: reads the file again, matches it against the list's
 * prices as they are now and writes the changes on the step's lines.
 */
export async function applyPriceImport(input: {
  list: string;
  upload: string;
  step: ImportStep;
}): Promise<ImportStepResult> {
  const client = await sessionClient();
  const read = await readImport(client, input.list, input.upload);
  if ("error" in read) return { failed: [], error: read.error };
  const { preview } = await matchImport(client, read.list, read.text, input.step);
  if (preview.error) return { failed: [], error: preview.error };
  const diff = stepDiff(preview.rows, preview.diff, input.step);
  if (diff.upserts.length === 0 && diff.clears.length === 0) return { failed: [] };
  const result = await saveFixedPrices(read.list.id, diff);
  return { failed: stepFailures(preview.rows, diff, result) };
}
