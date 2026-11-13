// Countries as the admin shows them: a flag and the name as the staff member's locale
// has it (`Intl.DisplayNames`), falling back to the API's English name. Shared by server
// and client components: each passes its `Format`'s locale (`format.locale`).

import type { AdminSchemas } from "@hyphacommerce/sdk/admin";

import { compareIn, displayName } from "./format.ts";
import type { KeyIn } from "./messages";

export type Country = AdminSchemas["CountryResponse"];

type Option = { value: string; label: string };

/** The flag emoji of a two-letter code (its regional indicator symbols). */
export function flag(code: string): string {
  const letters = code.toUpperCase();
  const symbol = (i: number) => 0x1f1e6 + letters.charCodeAt(i) - 65;
  return String.fromCodePoint(symbol(0), symbol(1));
}

/** "Croatia": `Intl`'s name in `locale`, else the API's, else the code itself. */
export function countryName(
  code: string,
  countries: Country[] | undefined,
  locale: string,
): string {
  return (
    displayName("region", code, locale) ?? countries?.find((c) => c.code === code)?.name ?? code
  );
}

/** "🇭🇷 Croatia", for selects and lists. */
export function countryLabel(
  code: string,
  countries: Country[] | undefined,
  locale: string,
): string {
  return `${flag(code)} ${countryName(code, countries, locale)}`;
}

/** "EUR · Euro" for each code, as select options. */
export function currencyOptions(codes: string[], locale: string): Option[] {
  return codes.map((code) => {
    const name = displayName("currency", code, locale);
    return { value: code, label: name ? `${code} · ${name}` : code };
  });
}

/** Every country as a select option, sorted by name as `locale` orders them. */
export function countryOptions(countries: Country[], locale: string): Option[] {
  return countries
    .map((c) => ({
      value: c.code,
      name: countryName(c.code, countries, locale),
    }))
    .toSorted((a, b) => compareIn(locale, a.name, b.name))
    .map(({ value, name }) => ({ value, label: `${flag(value)} ${name}` }));
}

/** The countries `markets` sell to, each with its market's name, leaving out `except`. */
export function takenCountries(
  markets: { id: string; name: string; countries: string[] }[],
  except?: string,
): Record<string, string> {
  return Object.fromEntries(
    markets.filter((m) => m.id !== except).flatMap((m) => m.countries.map((c) => [c, m.name])),
  );
}

/** Groups of countries for picking many at once; `label` is a message key (`t(region.label)`). */
export const REGIONS: { key: string; label: KeyIn<"components">; codes: string[] }[] = [
  {
    key: "eu",
    label: "components.region_eu",
    codes: [
      "AT",
      "BE",
      "BG",
      "HR",
      "CY",
      "CZ",
      "DK",
      "EE",
      "FI",
      "FR",
      "DE",
      "GR",
      "HU",
      "IE",
      "IT",
      "LV",
      "LT",
      "LU",
      "MT",
      "NL",
      "PL",
      "PT",
      "RO",
      "SK",
      "SI",
      "ES",
      "SE",
    ],
  },
  {
    key: "europe",
    label: "components.region_europe",
    codes: [
      "AL",
      "AD",
      "AT",
      "BY",
      "BE",
      "BA",
      "BG",
      "HR",
      "CY",
      "CZ",
      "DK",
      "EE",
      "FO",
      "FI",
      "FR",
      "DE",
      "GI",
      "GR",
      "GG",
      "HU",
      "IS",
      "IE",
      "IM",
      "IT",
      "JE",
      "LV",
      "LI",
      "LT",
      "LU",
      "MT",
      "MD",
      "MC",
      "ME",
      "NL",
      "MK",
      "NO",
      "PL",
      "PT",
      "RO",
      "SM",
      "RS",
      "SK",
      "SI",
      "ES",
      "SE",
      "CH",
      "UA",
      "GB",
      "VA",
    ],
  },
  { key: "north-america", label: "components.region_north_america", codes: ["US", "CA", "MX"] },
];
