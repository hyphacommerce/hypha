"use server";

import { revalidatePath } from "next/cache";

import { describe, sessionClient } from "@/lib/hypha";
import { type StoreLanguage, marketsInUse } from "@/lib/languages";

/** A market that still offers a language the change would take away. */
export type BlockingMarket = { id: string; name: string; languages: string[] };

/** The outcome of a change: a message to show if it failed, with the markets in its way. */
export type LanguagesResult = { error?: string; markets?: BlockingMarket[] };

/**
 * Changes the store's languages. `languages` is the whole list: the API replaces it, and
 * a language left out loses its translations for good, so callers send every language
 * they mean to keep.
 */
export async function saveLanguages(change: {
  defaultLanguage?: string;
  languages?: StoreLanguage[];
}): Promise<LanguagesResult> {
  const client = await sessionClient();
  const { error } = await client.PATCH("/admin/store", {
    body: { default_language: change.defaultLanguage, languages: change.languages },
  });
  if (error) {
    const message = await describe(error);
    if (error.code !== "language_in_use") return { error: message };
    // The answer names the markets by ID; their names, where the role reads markets.
    const blocking = marketsInUse(error.details);
    const { data } = blocking.length > 0 ? await client.GET("/admin/markets") : {};
    return {
      error: message,
      markets: blocking.map((market) => ({
        ...market,
        name: data?.items.find((m) => m.id === market.id)?.name ?? market.id,
      })),
    };
  }
  // The store is the layout's read; markets offer from this list.
  revalidatePath("/", "layout");
  return {};
}
