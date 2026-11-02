"use server";

import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { revalidatePath } from "next/cache";

import type { RuleBody, TestResult } from "@/lib/checkout-rules";
import { describe, sessionClient } from "@/lib/hypha";

/** A rule's broken parts, by where they are (`conditions[0].pattern`). */
export type RuleProblems = Record<string, string>;

/** The outcome of an edit: a message to show, if it failed, and each part's problem. */
export type Result = { error?: string; problems?: RuleProblems; id?: string };

function problemsOf(error: { code: string; details?: unknown }): RuleProblems | undefined {
  if (error.code !== "invalid_checkout_rule") return undefined;
  const details = error.details;
  if (typeof details !== "object" || details === null) return undefined;
  const list: unknown = Reflect.get(details, "problems");
  if (!Array.isArray(list)) return undefined;
  const problems: RuleProblems = {};
  for (const item of list) {
    if (typeof item !== "object" || item === null) continue;
    const path: unknown = Reflect.get(item, "path");
    const message: unknown = Reflect.get(item, "message");
    if (typeof path === "string" && typeof message === "string") problems[path] ??= message;
  }
  return problems;
}

function failed(error: { code: string; error: string; details?: unknown }): Result {
  const problems = problemsOf(error);
  return problems
    ? { error: "Check the highlighted parts of the rule.", problems }
    : { error: describe(error) };
}

function refresh() {
  revalidatePath("/settings/checkout", "layout");
}

export async function createRule(body: RuleBody): Promise<Result> {
  const client = await sessionClient();
  const { data, error } = await client.POST("/admin/checkout-rules", { body });
  if (error) return failed(error);
  refresh();
  return { id: data.id };
}

export async function updateRule(id: string, body: RuleBody): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.PATCH("/admin/checkout-rules/{id}", {
    params: { path: { id } },
    body,
  });
  if (error) return failed(error);
  refresh();
  return { id };
}

export async function setRuleEnabled(id: string, enabled: boolean): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.PATCH("/admin/checkout-rules/{id}", {
    params: { path: { id } },
    body: { enabled },
  });
  if (error) return { error: describe(error) };
  refresh();
  return {};
}

/** Moves a rule to `position` in the list; the others make room. */
export async function moveRule(id: string, position: number): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.PATCH("/admin/checkout-rules/{id}", {
    params: { path: { id } },
    body: { position },
  });
  if (error) return { error: describe(error) };
  refresh();
  return {};
}

export async function deleteRule(id: string): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.DELETE("/admin/checkout-rules/{id}", {
    params: { path: { id } },
  });
  if (error) return { error: describe(error) };
  refresh();
  return {};
}

/** What a cart would show for the address, with the draft rule tried in its place. */
export async function tryAddress(
  body: AdminSchemas["TestCheckoutRulesRequest"],
): Promise<{ result?: TestResult } & Result> {
  const client = await sessionClient();
  const { data, error } = await client.POST("/admin/checkout-rules/test", { body });
  if (error) return failed(error);
  return { result: data };
}

/** An app the checkout settings name. */
export type AppSetting = "verification_app" | "autocomplete_app";

/** Names the app verifying shipping addresses, or the one suggesting them; `null` for
 * none. */
export async function setCheckoutApp(setting: AppSetting, app: string | null): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.PATCH("/admin/checkout-settings", {
    body: { [setting]: app },
  });
  if (error) return { error: describe(error) };
  refresh();
  return {};
}
