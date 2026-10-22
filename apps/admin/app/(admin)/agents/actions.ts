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

const GRANT_FIELDS: GrantField[] = ["read", "write", "customer_data", "operations"];

/** A string among an error's `details`, if the API sent one under `key`. */
function detail(error: ApiError, key: string): string | undefined {
  const { details } = error;
  if (typeof details !== "object" || details === null) return undefined;
  const value: unknown = Reflect.get(details, key);
  return typeof value === "string" ? value : undefined;
}

/**
 * Where a save's error belongs: grant errors name their list (`details.field`) and the
 * scope they're about; a missing or long name is the name's.
 */
function placed(error: ApiError): SaveResult {
  const message = describe(error);
  const field = GRANT_FIELDS.find((f) => f === detail(error, "field"));
  if (field) return { error: message, field, scope: detail(error, "scope") };
  if (
    (error.code === "field_required" || error.code === "field_too_long") &&
    error.error.includes("name")
  ) {
    return { error: message, field: "name" };
  }
  return { error: message };
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
 * went from the proposal itself: `applied` or `failed` with the write's answer, or, when
 * the store changed since (`409 preview_stale`), still pending with what the write would
 * do now and `refreshes` counted, to approve again. So only an approval that didn't get
 * that far is an error here.
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

/** What answering a client's connection gave: where the browser goes now. */
export type ConnectResult = SaveResult & { redirectTo?: string };

/**
 * Connects an MCP client as an agent acting for the signed-in staff member, with
 * `grant`: the API answers the client's redirect URI with its code, for the browser.
 */
export async function approveConnection(
  id: string,
  name: string,
  grant: Grant,
): Promise<ConnectResult> {
  const client = await sessionClient();
  const { data, error } = await client.POST("/admin/agent-connections/{id}/approve", {
    params: { path: { id } },
    body: { name, grant },
  });
  if (error?.code === "oauth_request_expired") {
    return {
      error: "This request expired: requests wait ten minutes. Connect again from the client.",
    };
  }
  if (error) return placed(error);
  revalidatePath("/agents");
  return { redirectTo: data.redirect_to };
}

/** Declines a client's connection: the client is told `access_denied`. */
export async function denyConnection(id: string): Promise<ConnectResult> {
  const client = await sessionClient();
  const { data, error } = await client.POST("/admin/agent-connections/{id}/deny", {
    params: { path: { id } },
  });
  if (error) return { error: describe(error) };
  return { redirectTo: data.redirect_to };
}
