"use client";

// The currency switcher (design note 0024, §7). The shopper's country is found for them
// (the `country` cookie, else the host's geolocation, in `proxy.ts`), so the header only
// offers the currencies the store sells in, as Shopify themes do. Picking one moves the
// shopper to a country that pays in it (theirs, if it does) and remembers it.

import { Selector } from "@astryxdesign/core/Selector";
import { usePathname } from "next/navigation";
import { useTransition } from "react";

import { setCountry } from "@/app/[country]/actions";

import { useNavigationProgress } from "./navigation-progress";

export type CountryOption = { code: string; name: string; currency: string };

export function CurrencySwitcher({
  current,
  countries,
}: {
  current: string;
  countries: CountryOption[];
}) {
  const path = usePathname();
  const [pending, start] = useTransition();
  const progress = useNavigationProgress();
  const currency = countries.find((c) => c.code === current)?.currency;
  const currencies = [...new Set(countries.map((c) => c.currency))].toSorted();
  if (currencies.length < 2) return null;
  return (
    <Selector
      label="Currency"
      isLabelHidden
      variant="ghost"
      size="sm"
      isLoading={pending}
      value={currency}
      options={currencies.map((code) => ({ value: code, label: code }))}
      onChange={(picked) => {
        if (!picked || picked === currency) return;
        const target = countries.find((c) => c.currency === picked);
        if (target) {
          // The action redirects: the navigation bar is started by hand.
          progress.start();
          start(() => setCountry({ country: target.code, path }));
        }
      }}
    />
  );
}
