// An agent's grant as the admin's forms edit it, and back into what the API takes. Plain
// functions, so server pages (the consent page's preset) and client forms share them.

import type { AdminSchemas } from "@hypha/sdk/admin";

import type { WriteMode } from "@/lib/agents";

import type { Grant, GrantField } from "./actions";

/** A grant being edited; `operations` is the allowlist as typed, one per line. */
export type GrantDraft = {
  read: string[];
  write: Record<string, WriteMode>;
  customerData: string[];
  /** `null`: no allowlist, every operation the scopes allow. */
  operations: string | null;
};

export const EMPTY_GRANT: GrantDraft = { read: [], write: {}, customerData: [], operations: null };

/** A saved grant as a draft. */
export function draftOf(grant: AdminSchemas["AgentGrantResponse"]): GrantDraft {
  return {
    read: grant.read,
    write: Object.fromEntries(
      Object.entries(grant.write).map(([scope, mode]) => [
        scope,
        mode === "apply" ? "apply" : "propose",
      ]),
    ),
    customerData: grant.customer_data,
    operations: grant.operations ? grant.operations.join("\n") : null,
  };
}

/** The allowlist as the API takes it: one ID per line, blanks dropped. */
export function operationsOf(draft: GrantDraft): string[] | null {
  if (draft.operations === null) return null;
  return draft.operations
    .split(/\s+/)
    .map((op) => op.trim())
    .filter(Boolean);
}

/** The draft as the API takes a grant. */
export function grantOf(draft: GrantDraft): Grant {
  return {
    read: draft.read,
    write: draft.write,
    customer_data: draft.customerData,
    operations: operationsOf(draft),
  };
}

const sameSet = (a: readonly string[], b: readonly string[]) =>
  a.length === b.length && a.every((x) => b.includes(x));

/** Whether two drafts grant the same scopes in the same modes (allowlists aside). */
export function sameScopes(a: GrantDraft, b: GrantDraft): boolean {
  const aw = Object.entries(a.write);
  return (
    sameSet(a.read, b.read) &&
    sameSet(a.customerData, b.customerData) &&
    aw.length === Object.keys(b.write).length &&
    aw.every(([scope, mode]) => b.write[scope] === mode)
  );
}

/** Whether two drafts have the same allowlist. */
export function sameOperations(a: GrantDraft, b: GrantDraft): boolean {
  const ao = operationsOf(a);
  const bo = operationsOf(b);
  return ao === null || bo === null ? ao === bo : sameSet(ao, bo);
}

export type GrantProblem = { error: string; field?: GrantField; scope?: string };
