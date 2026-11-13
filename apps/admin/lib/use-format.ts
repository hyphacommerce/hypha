"use client";

// Formats for client components: numbers, money, dates, lists and names in the staff
// member's locale, with the store's currency table and time zone from the admin layout.
//
//   const format = useFormat();
//   format.money(order.total);      // "€18.00"
//   format.number(1234.5);          // "1,234.5"
//   format.dateTime(order.created_at, "short");
//
// Server components and actions call `getFormat()` (`lib/get-format.ts`) instead.

import { useT } from "@hyphacommerce/i18n/react";
import { useMemo } from "react";

import { useCurrenciesIfAny } from "@/components/currencies";
import { useTimeZone } from "@/components/date-time";

import { type Format, createFormat } from "./format";

export type { Format };

/**
 * The staff member's formats. Outside the admin layout (the login pages) there is no
 * currency table, so `money` throws there, and times are UTC.
 */
export function useFormat(): Format {
  const { locale } = useT();
  const currencies = useCurrenciesIfAny();
  const timeZone = useTimeZone();
  return useMemo(
    () => createFormat(locale, { currencies, timeZone }),
    [locale, currencies, timeZone],
  );
}
