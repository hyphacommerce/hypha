// Server-only: formats for server components and actions behind the login, in the staff
// member's locale, with the store's currency table and time zone.
//
//   const format = await getFormat();
//   format.money(order.total);
//
// Client components call `useFormat()` (`lib/use-format.ts`) instead.

import { type Format, createFormat } from "./format";
import { loadCurrencies, loadTimeZone } from "./hypha";
import { getT } from "./i18n";

export type { Format };

/**
 * The request's formats. It reads what the admin layout already read (the currency table
 * and the store, shared within the request), so a page's shell may await it. Only behind
 * the login: without a session those reads go to the login page. A page before it formats
 * with `createFormat((await getT()).locale)`.
 */
export async function getFormat(): Promise<Format> {
  const [t, currencies, timeZone] = await Promise.all([getT(), loadCurrencies(), loadTimeZone()]);
  return createFormat(t.locale, { currencies, timeZone });
}
