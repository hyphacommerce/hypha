"use server";

// Countries' address forms, and address autocomplete for the admin's address forms (customers' address books, an
// order's shipping address): the store's autocomplete app, through the API, so its
// provider's keys stay with the app.

import {
  type AddressFormat,
  type AddressValues,
  type FieldProblems,
  problemsByField,
} from "@/lib/address";
import { sessionClient } from "@/lib/hypha";

/** An address the autocomplete app offers for what staff typed into line 1. */
export type AddressSuggestion = { id: string; label: string; description: string | null };

/**
 * Addresses for `q`, typed into a form for `country` (none when the store has no app, it
 * fails, or too little was typed). `session` is the form's token for this address, the
 * same until one is picked.
 */
export async function suggestAddresses(input: {
  country: string;
  q: string;
  session: string;
}): Promise<AddressSuggestion[]> {
  const q = input.q.trim();
  if (q.length < 3 || q.length > 200) return [];
  const client = await sessionClient();
  const { data } = await client.GET("/admin/address-suggestions", {
    params: { query: { q, country: input.country, session: input.session } },
  });
  return (data?.suggestions ?? []).map((s) => ({
    id: s.id,
    label: s.label,
    description: s.description ?? null,
  }));
}

/** A picked suggestion as a form's values, with what saving it would refuse. */
export type ResolvedAddress = { values: Partial<AddressValues>; problems: FieldProblems };

/** The picked suggestion's address, or `null` when the app couldn't give it (staff type
 * it then). */
export async function resolveAddress(input: {
  country: string;
  id: string;
  session: string;
}): Promise<ResolvedAddress | null> {
  const client = await sessionClient();
  const { data } = await client.GET("/admin/address-suggestions/{id}", {
    params: { path: { id: input.id }, query: { country: input.country, session: input.session } },
  });
  if (!data) return null;
  const a = data.address;
  const problems = problemsByField(data.problems);
  return {
    values: {
      company: a.company ?? "",
      line1: a.line1,
      line2: a.line2 ?? "",
      city: a.city,
      province: a.province ?? "",
      postal_code: a.postal_code ?? "",
    },
    problems,
  };
}

/** A country's address form, for a form once a country is chosen. */
export async function loadAddressFormat(country: string): Promise<AddressFormat | null> {
  const client = await sessionClient();
  const { data } = await client.GET("/admin/address-formats/{country}", {
    params: { path: { country } },
  });
  return data ?? null;
}
