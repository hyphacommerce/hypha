"use client";

// The header's country picker (design note 0023, §2): sets the country and goes to the
// same page there. Names in the shopper's language where the browser knows them.

import { usePathname } from "next/navigation";
import { useRef } from "react";

import { setCountry } from "@/app/[country]/actions";

export function CountryPicker({
  current,
  countries,
}: {
  current: string;
  countries: { code: string; name: string; currency: string }[];
}) {
  const path = usePathname();
  const form = useRef<HTMLFormElement>(null);
  return (
    <form action={setCountry} ref={form}>
      <input type="hidden" name="path" value={path} />
      <label>
        <span className="muted" style={{ marginRight: 6, fontSize: "0.85rem" }}>
          Ship to
        </span>
        <select
          name="country"
          defaultValue={current}
          aria-label="Country"
          onChange={() => form.current?.requestSubmit()}
        >
          {countries.map((c) => (
            <option key={c.code} value={c.code}>
              {c.name} ({c.currency})
            </option>
          ))}
        </select>
      </label>
    </form>
  );
}
