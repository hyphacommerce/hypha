"use server";

// Price lists: their settings, the markets they're on, and their fixed prices (the
// grid's saves and CSV imports).

import { revalidatePath } from "next/cache";
import { redirect } from "next/navigation";

import { describe, loadCurrencies, loadTimeZone, sessionClient } from "@/lib/hypha";
import {
  type ImportPreview,
  type ImportRow,
  diffRequests,
  previewPriceImport,
} from "@/lib/price-csv";
import type { FixedPriceUpsert, PriceGridDiff } from "@/lib/price-grid/types";
import {
  type FormErrors,
  type ListForm,
  catalogsInUse,
  createRequest,
  entryErrors,
  updateRequest,
} from "@/lib/price-lists";

import { allGridProducts } from "./grid-data";

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

/** What saving fixed prices answers: the grid's `PriceGridSaveResult`. */
export type SaveResult = { error?: string; rowErrors?: Record<string, string> };

async function putPrices(client: Client, list: string, entries: readonly FixedPriceUpsert[]) {
  return client.PUT("/admin/price-lists/{id}/fixed-prices", {
    params: { path: { id: list } },
    body: {
      fixed_prices: entries.map((e) => ({
        variant: e.variant,
        price: e.price,
        compare_at: e.compare_at,
        breaks: e.breaks,
      })),
    },
  });
}

/**
 * Writes `diff`: a `PUT` per 1,000 fixed prices and a `DELETE` per 1,000 cleared ones.
 * A `PUT` is all or nothing, so one refused for some entries is sent again without
 * them, and only those keep their drafts. Every request is tried, so one failing
 * doesn't hold up the rest. One at a time, on purpose: each can carry 1,000 prices,
 * and the API checks each request whole.
 */
async function writeDiff(client: Client, list: string, diff: PriceGridDiff): Promise<SaveResult> {
  const { upserts, clears } = diffRequests(diff);
  const rowErrors: Record<string, string> = {};
  let error: string | undefined;
  for (const entries of upserts) {
    const first = await putPrices(client, list, entries);
    if (!first.error) continue;
    const refused =
      first.error.code === "fixed_prices_invalid"
        ? entryErrors(first.error.details, entries)
        : null;
    if (!refused || Object.keys(refused).length === 0) {
      for (const e of entries) rowErrors[e.variant] = describe(first.error);
      error ??= describe(first.error);
      continue;
    }
    Object.assign(rowErrors, refused);
    const rest = entries.filter((e) => !(e.variant in refused));
    if (rest.length === 0) continue;
    const second = await putPrices(client, list, rest);
    if (second.error) {
      const again =
        second.error.code === "fixed_prices_invalid"
          ? entryErrors(second.error.details, rest)
          : null;
      for (const e of rest) rowErrors[e.variant] = again?.[e.variant] ?? describe(second.error);
    }
  }
  for (const variants of clears) {
    const { error: e } = await client.DELETE("/admin/price-lists/{id}/fixed-prices", {
      params: { path: { id: list } },
      body: { variants },
    });
    if (e) {
      for (const v of variants) rowErrors[v] = describe(e);
      error ??= describe(e);
    }
  }
  refresh();
  return Object.keys(rowErrors).length > 0 ? { error, rowErrors } : {};
}

/** The grid's Save. */
export async function saveFixedPrices(list: string, diff: PriceGridDiff): Promise<SaveResult> {
  const client = await sessionClient();
  return writeDiff(client, list, diff);
}

/** The most rows an import's preview lists; the counts cover them all. */
const PREVIEW_ROWS = 500;

export type ImportPreviewResult = Omit<ImportPreview, "rows"> & {
  /** The rows that do something or can't (unchanged ones left out), up to 500. */
  rows: ImportRow[];
  /** How many such rows were left out of `rows`. */
  more: number;
};

/**
 * What importing `text` (a CSV file's contents) into the list would do, matched against
 * every product: nothing is written. Its `diff` is what `applyImportChunk` sends.
 */
export async function previewImport(list: string, text: string): Promise<ImportPreviewResult> {
  const client = await sessionClient();
  const [read, categories, currencies] = await Promise.all([
    client.GET("/admin/price-lists/{id}", { params: { path: { id: list } } }),
    client.GET("/admin/categories"),
    loadCurrencies(),
  ]);
  const empty = { changed: 0, unchanged: 0, unmatched: 0, invalid: 0 };
  if (!read.data) {
    return {
      rows: [],
      more: 0,
      counts: empty,
      diff: { upserts: [], clears: [] },
      error: read.error ? describe(read.error) : "The list couldn't be read.",
    };
  }
  const products = await allGridProducts(client, read.data, categories.data?.items ?? [], {
    fixedOnly: false,
  });
  const preview = previewPriceImport(text, products, read.data.currency, currencies);
  const notable = preview.rows.filter((r) => r.status !== "unchanged");
  return {
    ...preview,
    rows: notable.slice(0, PREVIEW_ROWS),
    more: Math.max(0, notable.length - PREVIEW_ROWS),
  };
}

/**
 * One step of applying an import: at most 1,000 fixed prices to set and 1,000 to
 * remove, so the dialog can show its progress between steps.
 */
export async function applyImportChunk(list: string, diff: PriceGridDiff): Promise<SaveResult> {
  const client = await sessionClient();
  return writeDiff(client, list, diff);
}
