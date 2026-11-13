// What's left to translate: each language's progress as the bar and its numbers show it,
// the worklist's filters read from its URL into the API's query, and how a resource in
// it is named. Plain functions, on both sides and under `node --test`.

import type { AdminSchemas } from "@hyphacommerce/sdk/admin";

import { sameTag } from "./languages.ts";
import type { TFor } from "./messages";
import { type Kind, isKind } from "./translations.ts";

export type LanguageProgress = AdminSchemas["LanguageProgressResponse"];
export type WorklistItem = AdminSchemas["TranslationSummaryResponse"];

/** The worklist's states, in the order its control offers them. */
export const STATES = ["missing", "outdated", "complete"] as const;
export type WorklistState = (typeof STATES)[number];

/** The state the worklist opens on: what's still to translate. */
export const DEFAULT_STATE: WorklistState = "missing";

export function isState(value: string | undefined): value is WorklistState {
  return STATES.some((state) => state === value);
}

/** The worklist's address. */
export const WORKLIST_PATH = "/translations";

/** One language's progress, as the bar draws it and its legend says it. */
export type ProgressParts = {
  total: number;
  /** Translated from the source as it is now. */
  current: number;
  /** Translated from a source that changed since. */
  outdated: number;
  missing: number;
  /** The share translated (outdated included), 0 to 1; `null` with nothing to translate. */
  translated: number | null;
  /** The bar's segments, each 0 to 1 of its width, adding up to 1 (or 0 when empty). */
  shares: { current: number; outdated: number; missing: number };
};

/**
 * `progress` as parts. The API's counts hold `translated + missing = total` and
 * `outdated ≤ translated`; the parts are clamped to them anyway, so a bar never draws
 * past its track nor a negative segment.
 */
export function progressParts(progress: LanguageProgress): ProgressParts {
  const total = Math.max(0, progress.total);
  const translated = Math.min(total, Math.max(0, progress.translated));
  const outdated = Math.min(translated, Math.max(0, progress.outdated));
  const current = translated - outdated;
  const missing = total - translated;
  const share = (count: number) => (total === 0 ? 0 : count / total);
  return {
    total,
    current,
    outdated,
    missing,
    translated: total === 0 ? null : translated / total,
    shares: { current: share(current), outdated: share(outdated), missing: share(missing) },
  };
}

/**
 * The share translated as the legend writes it, a fraction in whole percents rounded
 * down: a language with one field missing never reads "100%". Worked out from the
 * counts, since a fraction times 100 isn't exact (0.29 × 100 floors to 28).
 */
export function shownShare(parts: Pick<ProgressParts, "current" | "outdated" | "total">): number {
  if (parts.total === 0) return 0;
  return Math.floor(((parts.current + parts.outdated) * 100) / parts.total) / 100;
}

/**
 * Whether no language has anything counted: the store's texts haven't been indexed yet
 * (the API counts nothing until its background job read them once after an upgrade), or
 * the store has no text to translate.
 */
export function isUncounted(languages: readonly LanguageProgress[]): boolean {
  return languages.length > 0 && languages.every((language) => language.total === 0);
}

/** What the worklist shows, read from its URL against the store's other languages. */
export type WorklistFilters = {
  /** One of `languages`, in its canonical case; `undefined` when the store has none. */
  language: string | undefined;
  state: WorklistState;
  kind: Kind | undefined;
};

/**
 * The worklist's filters from its URL's values. A language the store doesn't have, or
 * none, is its first; an unknown state is the default one; an unknown kind is every
 * kind. So a hand-edited or outdated link still opens a list.
 */
export function worklistFilters(
  asked: { language?: string; state?: string; kind?: string },
  languages: readonly string[],
): WorklistFilters {
  const language =
    languages.find((tag) => asked.language !== undefined && sameTag(tag, asked.language)) ??
    languages[0];
  return {
    language,
    state: isState(asked.state) ? asked.state : DEFAULT_STATE,
    kind: asked.kind !== undefined && isKind(asked.kind) ? asked.kind : undefined,
  };
}

/** The API's query for a page of the worklist. */
export function worklistQuery(
  filters: WorklistFilters & { language: string },
  page: { cursor?: string; limit?: number },
): { language: string; state: WorklistState; kind?: Kind; cursor?: string; limit?: number } {
  return {
    language: filters.language,
    state: filters.state,
    ...(filters.kind === undefined ? {} : { kind: filters.kind }),
    ...(page.cursor === undefined ? {} : { cursor: page.cursor }),
    ...(page.limit === undefined ? {} : { limit: page.limit }),
  };
}

/** The worklist opened on `language`, and on a state and a kind when given. */
export function worklistHref(filters: {
  language: string;
  state?: WorklistState;
  kind?: Kind;
}): string {
  const query = new URLSearchParams({ language: filters.language });
  if (filters.state !== undefined) query.set("state", filters.state);
  if (filters.kind !== undefined) query.set("kind", filters.kind);
  return `${WORKLIST_PATH}?${query.toString()}`;
}

/** A kind's name, or the API's word for a kind this build doesn't know. */
export function kindLabel(t: TFor<"translations">, kind: string): string {
  return isKind(kind) ? t(`translations.kind_${kind}`) : kind;
}

/**
 * Kinds a store has one of (its details, its search filters): the kind's name is the
 * resource's name, and its ID (the store's) says nothing.
 */
export function isStoreWide(kind: string): boolean {
  return kind === "store" || kind === "search_facet";
}

/**
 * Kinds whose names the worklist reads in one call per page: products by their IDs, and
 * the kinds the API lists whole. Discounts and files are listed only a page at a time,
 * with no filter by ID, so they're shown by kind and a short ID.
 */
export const NAMED_KINDS = [
  "product",
  "category",
  "custom_field_definition",
  "shipping_option",
  "payment_method",
  "checkout_rule",
] as const satisfies readonly Kind[];
export type NamedKind = (typeof NAMED_KINDS)[number];

function isNamedKind(kind: string): kind is NamedKind {
  return NAMED_KINDS.some((named) => named === kind);
}

/** The IDs of a page's resources whose names can be read, by kind, each once. */
export function idsToName(items: readonly WorklistItem[]): Partial<Record<NamedKind, string[]>> {
  const ids: Partial<Record<NamedKind, string[]>> = {};
  for (const { resource } of items) {
    if (!isNamedKind(resource.kind)) continue;
    const list = (ids[resource.kind] ??= []);
    if (!list.includes(resource.id)) list.push(resource.id);
  }
  return ids;
}

/** Where names are kept: `"<kind>:<id>"`, since an ID is unique only within its kind. */
export function nameKey(kind: string, id: string): string {
  return `${kind}:${id}`;
}
