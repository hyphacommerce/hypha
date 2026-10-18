"use server";

import { revalidatePath } from "next/cache";

import { describe, sessionClient } from "@/lib/hypha";

/** The outcome of an edit: a message to show, if it failed. */
export type Result = { error?: string };

/** A method as the form gives it. Empty markets or channels: every one. */
export type MethodInput = {
  name: string;
  instructions: string;
  markets: string[];
  channels: string[];
  enabled: boolean;
};

/** The setup checklist (home page, wizard) counts enabled methods: refresh it too. */
function refresh() {
  revalidatePath("/", "layout");
}

/** Blank instructions are none. */
function instructions(text: string): string | null {
  return text.trim() === "" ? null : text.trim();
}

/** A manual method (staff record the money), after the others. */
export async function createPaymentMethod(input: MethodInput): Promise<Result> {
  if (input.name.trim() === "") return { error: "The method needs a name." };
  const client = await sessionClient();
  const { error } = await client.POST("/admin/payment-methods", {
    body: {
      provider: "manual",
      name: input.name.trim(),
      instructions: instructions(input.instructions),
      markets: input.markets,
      channels: input.channels,
      enabled: input.enabled,
    },
  });
  if (error) return { error: describe(error) };
  refresh();
  return {};
}

export async function updatePaymentMethod(id: string, input: MethodInput): Promise<Result> {
  if (input.name.trim() === "") return { error: "The method needs a name." };
  const client = await sessionClient();
  const { error } = await client.PATCH("/admin/payment-methods/{id}", {
    params: { path: { id } },
    body: {
      name: input.name.trim(),
      instructions: instructions(input.instructions),
      markets: input.markets,
      channels: input.channels,
      enabled: input.enabled,
    },
  });
  if (error) return { error: describe(error) };
  refresh();
  return {};
}

export async function setPaymentMethodEnabled(id: string, enabled: boolean): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.PATCH("/admin/payment-methods/{id}", {
    params: { path: { id } },
    body: { enabled },
  });
  if (error) return { error: describe(error) };
  refresh();
  return {};
}

/**
 * Puts the methods in `order` (every method's ID, as shoppers should see them): each
 * one whose position isn't its index gets it. Positions are a plain sort key, so
 * numbering them 0, 1, 2… also straightens out ties.
 */
export async function reorderPaymentMethods(
  order: { id: string; position: number }[],
): Promise<Result> {
  const client = await sessionClient();
  for (const [index, method] of order.entries()) {
    if (method.position === index) continue;
    const { error } = await client.PATCH("/admin/payment-methods/{id}", {
      params: { path: { id: method.id } },
      body: { position: index },
    });
    if (error) {
      refresh();
      return { error: describe(error) };
    }
  }
  refresh();
  return {};
}

export async function deletePaymentMethod(id: string): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.DELETE("/admin/payment-methods/{id}", {
    params: { path: { id } },
  });
  if (error) return { error: describe(error) };
  refresh();
  return {};
}
