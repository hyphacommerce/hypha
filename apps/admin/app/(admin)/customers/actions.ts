"use server";

// What the customer pages ask the API (design note 0032, §12 step 2): the customer's own
// fields, the address book, the labels in use, and the well-known fields' creation.

import type { AdminSchemas } from "@hypha/sdk/admin";
import { revalidatePath } from "next/cache";

import { describe, sessionClient } from "@/lib/hypha";
import { WELL_KNOWN, type WellKnown, suggestionBody } from "@/lib/well-known-fields";

/** The outcome of an edit: a message to show, if it failed. */
export type Result = { error?: string };

export type UpdateCustomerRequest = AdminSchemas["UpdateCustomerRequest"];
export type AddressRequest = AdminSchemas["AddressRequest"];
export type SetConsentRequest = AdminSchemas["SetConsentRequest"];
export type Channel = "email" | "sms";

function refresh(customer: string) {
  revalidatePath(`/customers/${customer}`);
  revalidatePath("/customers");
}

/** Changes the customer's email, name or phone (a JSON Merge Patch: `null` clears). */
export async function updateCustomer(id: string, body: UpdateCustomerRequest): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.PATCH("/admin/customers/{id}", { params: { path: { id } }, body });
  if (error) return { error: describe(error) };
  refresh(id);
  return {};
}

/** Adds an address to the book, or replaces `address` whole (defaults included). */
export async function saveAddress(
  customer: string,
  address: string | null,
  body: AddressRequest,
): Promise<Result> {
  const client = await sessionClient();
  const { error } = address
    ? await client.PUT("/admin/customers/{id}/addresses/{address_id}", {
        params: { path: { id: customer, address_id: address } },
        body,
      })
    : await client.POST("/admin/customers/{id}/addresses", {
        params: { path: { id: customer } },
        body,
      });
  if (error) return { error: describe(error) };
  refresh(customer);
  return {};
}

/**
 * Records a marketing consent as given now by staff (design note 0032, §5). `applied`
 * is false when a newer consent is stored: nothing changed then.
 */
export async function setConsent(
  id: string,
  channel: Channel,
  body: SetConsentRequest,
): Promise<Result & { applied?: boolean }> {
  const client = await sessionClient();
  const { data, error } = await client.PUT("/admin/customers/{id}/marketing/{channel}", {
    params: { path: { id, channel } },
    body,
  });
  if (error) return { error: describe(error) };
  refresh(id);
  return { applied: data.applied };
}

/** Asks for the customer's personal data to be erased after the cancel window (§7.3). */
/** Signs the customer out of every device (design note 0034, §7). */
export async function revokeSessions(id: string): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.POST("/admin/customers/{id}/sessions/revoke", {
    params: { path: { id } },
  });
  if (error) return { error: describe(error) };
  refresh(id);
  return {};
}

export async function requestErasure(id: string): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.POST("/admin/customers/{id}/erasure", {
    params: { path: { id } },
  });
  if (error) return { error: describe(error) };
  refresh(id);
  return {};
}

/** Cancels a requested erasure while the window is open. */
export async function cancelErasure(id: string): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.DELETE("/admin/customers/{id}/erasure", {
    params: { path: { id } },
  });
  if (error) return { error: describe(error) };
  refresh(id);
  return {};
}

/** Deletes a customer without orders (design note 0032, CD7); the API refuses otherwise. */
export async function deleteCustomer(id: string): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.DELETE("/admin/customers/{id}", { params: { path: { id } } });
  if (error) return { error: describe(error) };
  revalidatePath("/customers");
  return {};
}

export async function removeAddress(customer: string, address: string): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.DELETE("/admin/customers/{id}/addresses/{address_id}", {
    params: { path: { id: customer, address_id: address } },
  });
  if (error) return { error: describe(error) };
  refresh(customer);
  return {};
}

/**
 * The labels in use, most common first, for autocomplete. Empty when the session may
 * not read them (the field's owner type's read scope).
 */
export async function labelsInUse(definition: string): Promise<string[]> {
  const client = await sessionClient();
  const { data } = await client.GET("/admin/custom-field-definitions/{id}/values-in-use", {
    params: { path: { id: definition }, query: { limit: 100 } },
  });
  return (data?.items ?? [])
    .map((item) => item.value)
    .filter((value): value is string => typeof value === "string");
}

/**
 * Creates a well-known customer field from the API's suggestion (design note 0032,
 * CD11), as Settings → Custom fields would.
 */
export async function addWellKnownField(which: WellKnown): Promise<Result> {
  const client = await sessionClient();
  const suggestions = await client.GET("/admin/custom-field-definitions/suggestions", {
    params: { query: { owner_type: "customer" } },
  });
  if (suggestions.error) return { error: describe(suggestions.error) };
  const suggestion = suggestions.data.items.find((s) => s.key === WELL_KNOWN[which].key);
  if (!suggestion) return { error: "The API doesn't offer this field." };
  if (suggestion.existing) {
    return {
      error: `A field ${suggestion.field} exists already, with another shape. Find it under Settings → Custom fields.`,
    };
  }
  const { error } = await client.POST("/admin/custom-field-definitions", {
    body: suggestionBody(suggestion),
  });
  if (error) return { error: describe(error) };
  // Definitions shape every customer page and the list's filters.
  revalidatePath("/", "layout");
  return {};
}
