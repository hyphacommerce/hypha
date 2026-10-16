"use client";

// The currency table (`GET /admin/currencies`) for client components: the admin layout
// loads it once (`loadCurrencies`) and provides it here, so a form formats and parses
// money with the right digits without fetching anything itself.

import { createContext, useContext } from "react";

import type { Currencies } from "@/lib/format";

const CurrenciesContext = createContext<Currencies | null>(null);

export function CurrenciesProvider({
  currencies,
  children,
}: {
  currencies: Currencies;
  children: React.ReactNode;
}) {
  return <CurrenciesContext value={currencies}>{children}</CurrenciesContext>;
}

/** The currency table; only inside the admin layout, which provides it. */
export function useCurrencies(): Currencies {
  const currencies = useContext(CurrenciesContext);
  if (!currencies) throw new Error("useCurrencies outside the admin layout's CurrenciesProvider");
  return currencies;
}
