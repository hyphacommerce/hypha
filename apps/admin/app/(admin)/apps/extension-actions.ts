"use server";

// What the admin does for apps' UI and installation (design note 0029): answering a
// consent request, uploading a private app's manifest, a session token for an embedded
// page, and running an app's action. Each goes through the admin's server, so the staff
// session never reaches the browser's JavaScript or an app.

import type { AdminSchemas } from "@hypha/sdk/admin";
import { revalidatePath } from "next/cache";

import { appPagePath, loadExtensions } from "@/lib/app-extensions";
import { describe, sessionClient } from "@/lib/hypha";

/** A manifest problem, where and what (`details.errors` of `manifest_invalid`). */
export type ManifestProblem = { path: string; message: string };

export type Credentials = AdminSchemas["PrivateAppCredentials"];

export type ConsentResult =
  | { redirectTo: string }
  | { credentials: Credentials }
  | { done: true }
  | { error: string };

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function problemsOf(details: unknown): ManifestProblem[] {
  if (typeof details !== "object" || details === null) return [];
  const errors: unknown = Reflect.get(details, "errors");
  if (!Array.isArray(errors)) return [];
  return errors.flatMap((e: unknown) =>
    typeof e === "object" && e !== null
      ? [{ path: String(Reflect.get(e, "path")), message: String(Reflect.get(e, "message")) }]
      : [],
  );
}

/** Approves a request: where the browser goes next, or a private app's credentials. */
export async function approveConsent(id: string): Promise<ConsentResult> {
  const client = await sessionClient();
  const { data, error } = await client.POST("/admin/oauth/requests/{id}/approve", {
    params: { path: { id } },
  });
  if (error) return { error: describe(error) };
  revalidatePath("/apps");
  if (data.credentials) return { credentials: data.credentials };
  if (data.redirect_to) return { redirectTo: data.redirect_to };
  return { done: true };
}

/** Denies a request: where the browser goes next (nowhere for a private app). */
export async function denyConsent(id: string): Promise<ConsentResult> {
  const client = await sessionClient();
  const { data, error } = await client.POST("/admin/oauth/requests/{id}/deny", {
    params: { path: { id } },
  });
  if (error) return { error: describe(error) };
  revalidatePath("/apps");
  return data.redirect_to ? { redirectTo: data.redirect_to } : { done: true };
}

export type PrivateAppResult =
  | { request: string }
  | { error: string; problems?: ManifestProblem[] };

/**
 * Uploads a private app's manifest (`POST /admin/apps/private`): a request for the
 * consent page, or what's wrong with it.
 */
export async function createPrivateApp(manifestText: string): Promise<PrivateAppResult> {
  let manifest: unknown;
  try {
    manifest = JSON.parse(manifestText);
  } catch (e) {
    return { error: `That isn't JSON: ${e instanceof Error ? e.message : String(e)}` };
  }
  if (!isRecord(manifest)) return { error: "A manifest is a JSON object." };
  const client = await sessionClient();
  const { data, error } = await client.POST("/admin/apps/private", {
    body: { manifest },
  });
  if (error) return { error: describe(error), problems: problemsOf(error.details) };
  return { request: data.request.id };
}

/** A fresh session token for an app's embedded page (`docs/apps/bridge.md`). */
export async function appSessionToken(app: string): Promise<{ token?: string; error?: string }> {
  const client = await sessionClient();
  const { data, error } = await client.POST("/admin/app-extensions/{app}/session", {
    params: { path: { app } },
  });
  if (error) return { error: describe(error) };
  return { token: data.token };
}

export type ActionResult = {
  message?: string;
  error?: string;
  /** Re-render the page the action was pressed on. */
  refresh?: boolean;
  /** An admin path to go to (the app's page at the path it named). */
  navigate?: string;
};

export type ActionContext = AdminSchemas["ExtensionContextDto"];

/**
 * Runs an app's action (`POST /admin/app-extensions/actions/{app}/{id}`) on `context`,
 * with a form's `input` when it takes one.
 */
export async function runAppAction(
  app: string,
  id: string,
  context: ActionContext,
  input?: Record<string, unknown>,
): Promise<ActionResult> {
  const client = await sessionClient();
  const { data, error } = await client.POST("/admin/app-extensions/actions/{app}/{id}", {
    params: { path: { app, id } },
    body: { context, ...(input ? { input } : {}) },
  });
  if (error) return { error: describe(error) };
  let navigate: string | undefined;
  if (data.navigate) {
    navigate = appPagePath(await loadExtensions(), app, data.navigate) ?? undefined;
  }
  return { message: data.message ?? undefined, refresh: data.refresh, navigate };
}

/**
 * An action that names a payment point (`payments.refund`): the payment's own route,
 * which asks the app (202: accepted; the payment moves when the app reports it). A
 * refund's form may give an `amount` in minor units; without one, all of it.
 */
export async function runPaymentAction(
  point: string,
  paymentId: string,
  orderId: string,
  input?: Record<string, unknown>,
): Promise<ActionResult> {
  const client = await sessionClient();
  const params = { path: { id: paymentId } };
  // Where it stood, to tell whether the action moved it (a second partial refund leaves
  // it `partially_refunded` either way).
  const before = await client.GET("/admin/payments/{id}", { params });
  let result;
  switch (point) {
    case "payments.capture":
      result = await client.POST("/admin/payments/{id}/capture", { params, body: {} });
      break;
    case "payments.void":
      result = await client.POST("/admin/payments/{id}/void", { params, body: {} });
      break;
    case "payments.refund": {
      const amount = typeof input?.amount === "number" ? input.amount : null;
      result = await client.POST("/admin/payments/{id}/refund", { params, body: { amount } });
      break;
    }
    default:
      return { error: `This admin can't run ${point}.` };
  }
  if (result.error) return { error: describe(result.error) };
  revalidatePath(`/orders/${orderId}`);
  // A provider built in may answer that it's done already (a spore's capture Stripe
  // answered `succeeded`): the payment has moved, so say so.
  const status = result.data.payment.status;
  const moved =
    before.data?.status !== status ||
    before.data?.refunded.amount !== result.data.payment.refunded.amount;
  const done = moved && DONE[point]?.includes(status);
  return {
    message: done
      ? `Done: the payment is ${status.replace("_", " ")}.`
      : "Sent to the payment's app. The payment changes when it reports back.",
    refresh: true,
  };
}

/** The statuses that mean a payment action has happened. */
const DONE: Record<string, string[]> = {
  "payments.capture": ["paid"],
  "payments.void": ["voided"],
  "payments.refund": ["refunded", "partially_refunded"],
};
