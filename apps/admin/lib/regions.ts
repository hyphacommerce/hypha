// Countries as the admin shows them (design note 0021, SU2): a flag and the local name
// (`Intl.DisplayNames`), falling back to the API's English name. Shared by server and
// client components.

import type { AdminSchemas } from "@hypha/sdk/admin";

export type Country = AdminSchemas["CountryResponse"];

const regionNames = new Intl.DisplayNames("en", { type: "region", fallback: "none" });

/** The flag emoji of a two-letter code (its regional indicator symbols). */
export function flag(code: string): string {
  return String.fromCodePoint(...[...code.toUpperCase()].map((c) => 0x1f1e6 + c.charCodeAt(0) - 65));
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
    .sort((a, b) => countryName(a.value, countries).localeCompare(countryName(b.value, countries)));
}
