// Countries as the admin shows them: a flag and the local name (`Intl.DisplayNames`),
// falling back to the API's English name. Shared by server and client components.

import type { AdminSchemas } from "@hyphacommerce/sdk/admin";

export type Country = AdminSchemas["CountryResponse"];

const regionNames = new Intl.DisplayNames("en", { type: "region", fallback: "none" });

/** The flag emoji of a two-letter code (its regional indicator symbols). */
export function flag(code: string): string {
  const letters = code.toUpperCase();
  const symbol = (i: number) => 0x1f1e6 + letters.charCodeAt(i) - 65;
  return String.fromCodePoint(symbol(0), symbol(1));
}

/** "Croatia": `Intl`'s name, else the API's, else the code itself. */
export function countryName(code: string, countries?: Country[]): string {
  return regionNames.of(code) ?? countries?.find((c) => c.code === code)?.name ?? code;
}

/** "🇭🇷 Croatia", for selects and lists. */
export function countryLabel(code: string, countries?: Country[]): string {
  return `${flag(code)} ${countryName(code, countries)}`;
}

const currencyNames = new Intl.DisplayNames("en", { type: "currency", fallback: "none" });

/** "EUR · Euro" for each code, as select options. */
export function currencyOptions(codes: string[]): { value: string; label: string }[] {
  return codes.map((code) => {
    const name = currencyNames.of(code);
    return { value: code, label: name ? `${code} · ${name}` : code };
  });
}

/** Every country as a select option, sorted by name. */
export function countryOptions(countries: Country[]): { value: string; label: string }[] {
  return countries
    .map((c) => ({ value: c.code, label: countryLabel(c.code, countries) }))
    .toSorted((a, b) =>
      countryName(a.value, countries).localeCompare(countryName(b.value, countries)),
    );
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

/** Groups of countries for picking many at once. */
export const REGIONS: { key: string; label: string; codes: string[] }[] = [
  {
    key: "eu",
    label: "European Union",
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
    label: "Europe",
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
  { key: "north-america", label: "North America", codes: ["US", "CA", "MX"] },
];
