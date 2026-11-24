// Server-only: whether a page offers "Translate", and the read behind the editor. Never
// import this from a "use client" file.

import { loadStore, loaded, requireStaff, sessionClient } from "./hypha";
import {
  type Kind,
  type Translations,
  canReadTranslations,
  canWriteTranslations,
} from "./translations";

/**
 * Whether the signed-in staff member is offered "Translate" on a `kind`'s pages: their
 * role reads its translations, and the store has a language to translate into. Without
 * `stores:read` the list isn't known and the action shows; the editor then says what
 * there is.
 */
export async function offersTranslate(kind: Kind): Promise<boolean> {
  const [me, store] = await Promise.all([requireStaff(), loadStore()]);
  if (!canReadTranslations(me.scopes, kind)) return false;
  return store === null || store.languages.length > 0;
}

/** Whether they can save a `kind`'s translations. */
export async function mayTranslate(kind: Kind): Promise<boolean> {
  return canWriteTranslations((await requireStaff()).scopes, kind);
}

/**
 * A resource's translatable fields with every language's translation. A resource that
 * doesn't exist is the 404 page; one the role can't read, the no-access notice.
 */
export async function loadTranslations(kind: Kind, id: string): Promise<Translations> {
  const client = await sessionClient();
  return loaded(
    await client.GET("/admin/translations/{kind}/{id}", { params: { path: { kind, id } } }),
  );
}
