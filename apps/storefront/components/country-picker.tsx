"use client";

// The header's country picker (design note 0023, §2): sets the country and goes to the
// same page there. Names in the shopper's language where the server knew them.

import { Selector } from "@astryxdesign/core/Selector";
import { GlobeEuropeAfricaIcon } from "@heroicons/react/24/outline";
import { usePathname } from "next/navigation";
import { useTransition } from "react";

import { setCountry } from "@/app/[country]/actions";

export function CountryPicker({
  current,
  countries,
}: {
  current: string;
  countries: { code: string; name: string; currency: string }[];
}) {
  const path = usePathname();
  const [pending, start] = useTransition();
  return (
    <Selector
      label="Ship to"
      isLabelHidden
      variant="ghost"
      size="sm"
      startIcon={GlobeEuropeAfricaIcon}
      presentation="adaptive"
      hasSearch={countries.length > 8}
      isLoading={pending}
      value={current}
      options={countries.map((c) => ({
        value: c.code,
        label: `${c.name} (${c.currency})`,
      }))}
      renderValue={(option) => option.value}
      onChange={(country) => {
        if (country && country !== current) {
          start(() => setCountry({ country, path }));
        }
      }}
    />
  );
}
