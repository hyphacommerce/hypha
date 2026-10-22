"use server";

import type { AdminSchemas, ApiError } from "@hypha/sdk/admin";
import { revalidatePath } from "next/cache";

import { describe, sessionClient } from "@/lib/hypha";

export type Result = { error?: string };

/** The part of a grant form an error is about, so it shows there. */
export type GrantField = "name" | "read" | "write" | "customer_data" | "operations";

/** Why saving an agent failed, placed on its field (and scope) when the API says which. */
export type SaveResult = { error?: string; field?: GrantField; scope?: string };

/** A new agent and its key, shown once; or why it wasn't made. */
export type CreateResult = SaveResult & { id?: string; key?: string };

export type Grant = AdminSchemas["AgentGrantRequest"];

/**
 * Where a grant error belongs: the API's message names the scope it's about ("orders:write
 * is granted to an agent to propose only"), and the scope's kind says the list.
 */
function placed(error: ApiError): SaveResult {
  const message = describe(error);
  if (error.code === "invalid_operation_id" || error.code === "too_many_operations") {
    return { error: message, field: "operations" };
  }
  if (
    (error.code === "field_required" || error.code === "field_too_long") &&
    error.error.includes("name")
  ) {
    return { error: message, field: "name" };
  }
  const scope = /\b[a-z_]+:[a-z_]+\b/.exec(error.error)?.[0];
  if (!scope) return { error: message };
  const field: GrantField = scope.startsWith("customer_data:")
    ? "customer_data"
    : scope.endsWith(":read")
      ? "read"
      : "write";
  return { error: message, field, scope };
}

/** An autonomous agent with `grant`; its key comes back once. */
export async function createAgent(name: string, grant: Grant): Promise<CreateResult> {
  const client = await sessionClient();
  const { data, error } = await client.POST("/admin/agents", { body: { name, grant } });
  if (error) return placed(error);
  revalidatePath("/agents");
  return { id: data.id, key: data.key };
}

/**
 * Saves an agent's name, grant or allowlist: the grant only when its scopes changed (a
 * grant is checked against what the caller holds), else just the allowlist.
 */
export async function saveAgent(
  id: string,
  changes: { name?: string; grant?: Grant; operations?: string[] | null },
): Promise<SaveResult> {
  const client = await sessionClient();
  const { error } = await client.PATCH("/admin/agents/{id}", {
    params: { path: { id } },
    body: changes,
  });
  if (error) return placed(error);
  revalidatePath(`/agents/${id}`);
  revalidatePath("/agents");
  return {};
}

/** Turns an agent off (its credentials stop until it's on again) or back on. */
export async function setAgentEnabled(id: string, enabled: boolean): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.PATCH("/admin/agents/{id}", {
    params: { path: { id } },
    body: { enabled },
  });
  if (error) return { error: describe(error) };
  revalidatePath(`/agents/${id}`);
  revalidatePath("/agents");
  return {};
}

/** Revokes an agent for good: its credentials stop, its proposals can't be approved. */
export async function revokeAgent(id: string): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.POST("/admin/agents/{id}/revoke", { params: { path: { id } } });
  if (error) return { error: describe(error) };
  revalidatePath(`/agents/${id}`);
  revalidatePath("/agents");
  return {};
}

/**
 * Approves a proposal: its write runs as previewed, or not at all. The page shows how it
 * went from the proposal itself, `applied` or `failed` with the write's answer (a
 * `preview_stale` answer carries what the write would do now), so only an approval that
 * didn't get that far is an error here.
 */
export async function approveProposal(id: string): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.POST("/admin/agent-proposals/{id}/approve", {
    params: { path: { id } },
  });
  revalidatePath(`/agents/proposals/${id}`);
  revalidatePath("/agents/proposals");
  if (error && error.code !== "preview_stale") return { error: describe(error) };
  return {};
}

/** Rejects a proposal, with a note the agent can read. */
export async function rejectProposal(id: string, note: string): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.POST("/admin/agent-proposals/{id}/reject", {
    params: { path: { id } },
    body: { note: note.trim() || null },
  });
  if (error) return { error: describe(error) };
  revalidatePath(`/agents/proposals/${id}`);
  revalidatePath("/agents/proposals");
  return {};
}
