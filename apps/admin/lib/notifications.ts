// Notifications in the admin: what each template is called and does, how a message's
// status reads, and the variables a template can print. Plain functions, for server and
// client code alike: the ones that build text take the caller's translator.

import { hasKey } from "./keys.ts";
import type { TFor } from "./messages";

type NotificationsT = TFor<"notifications">;

/** The templates' names in English, for callers that have no translator yet. */
const ENGLISH_NAMES: Record<string, string> = {
  layout: "Email layout",
  order_confirmation: "Order confirmation",
  order_shipped: "Order shipped",
  order_tracking_updated: "Tracking updated",
  order_cancelled: "Order cancelled",
  staff_new_order: "New order",
  staff_invite: "Staff invite",
  staff_password_reset: "Password reset",
  customer_login_code: "Customer sign-in code",
  customer_erasure_requested: "Erasure requested",
  supplier_purchase_order: "Purchase order",
};

/**
 * A template key as people read it, in English.
 * @deprecated Pass the translator first: `templateName(t, key)`.
 */
export function templateName(key: string): string;
/** A template key as people read it; an unknown one as it is. */
export function templateName(t: NotificationsT, key: string): string;
export function templateName(first: NotificationsT | string, key?: string): string {
  if (typeof first === "string") return ENGLISH_NAMES[first] ?? first;
  if (key === undefined) return "";
  const name = `notifications.template_${key}_name`;
  return hasKey(first, name) ? first(name) : key;
}

/** What a template is for, by its stable key; nothing for an unknown one. */
export function templateDescription(t: NotificationsT, key: string): string | undefined {
  const description = `notifications.template_${key}_description`;
  return hasKey(t, description) ? t(description) : undefined;
}

/**
 * A message's status: its badge colour, and its label in English. With a translator,
 * the label is `messageStatusLabel(t, status)`.
 */
export const MESSAGE_STATUS: Record<
  string,
  { label: string; variant: "success" | "neutral" | "warning" | "error" | "info" }
> = {
  queued: { label: "Queued", variant: "info" },
  sending: { label: "Sending", variant: "info" },
  sent: { label: "Sent", variant: "success" },
  failed: { label: "Failed", variant: "error" },
  skipped: { label: "Skipped", variant: "neutral" },
};

/** A message's status as people read it; an unknown one as it is. */
export function messageStatusLabel(t: NotificationsT, status: string): string {
  const label = `notifications.message_status_${status}`;
  return hasKey(t, label) ? t(label) : status;
}

/** Which source a template problem is in, as the editor names it; an unknown one as it is. */
export function partName(t: NotificationsT, part: string): string {
  const name = `notifications.part_${part}`;
  return hasKey(t, name) ? t(name) : part;
}

/** Where a template problem is: its part, its line (from 1, when known) and the words. */
export type Problem = { part: string; line?: number | null; message: string };

/** The parts' names in English, for callers that have no `notifications` translator yet. */
const ENGLISH_PARTS: Record<string, string> = {
  subject: "Subject",
  html: "HTML",
  text: "Plain text",
  layout: "Email layout",
};

/**
 * "HTML, line 4: …" in English.
 * @deprecated Pass the translator first: `describeProblem(t, problem)`.
 */
export function describeProblem(problem: Problem): string;
/** "HTML, line 4: …": a problem for a banner. The words are the API's, as it wrote them. */
export function describeProblem(t: NotificationsT, problem: Problem): string;
export function describeProblem(first: NotificationsT | Problem, second?: Problem): string {
  if (typeof first !== "function") {
    const part = ENGLISH_PARTS[first.part] ?? first.part;
    return first.line
      ? `${part}, line ${first.line}: ${first.message}`
      : `${part}: ${first.message}`;
  }
  if (!second) return "";
  const part = partName(first, second.part);
  return second.line
    ? first("notifications.problem_at_line", {
        part,
        line: String(second.line),
        message: second.message,
      })
    : first("notifications.problem_in_part", { part, message: second.message });
}

/** A template problem among an error's `details`, if the API sent one. */
export function problemOf(details: unknown): Problem | undefined {
  if (typeof details !== "object" || details === null) return undefined;
  const part: unknown = Reflect.get(details, "part");
  const message: unknown = Reflect.get(details, "message");
  const line: unknown = Reflect.get(details, "line");
  if (typeof part !== "string" || typeof message !== "string") return undefined;
  return { part, message, line: typeof line === "number" ? line : null };
}

/** One variable a template can print: its dotted path and the sample's value. */
export type Variable = { path: string; sample: string; isList: boolean };

/**
 * Every printable path of a template's sample context, depth first: `order.total.formatted`
 * and the like. A list shows its first item's fields under `name[0]`, so a loop's
 * variable (`{% for line in order.lines %}{{ line.title }}`) is easy to find. Paths and
 * sample values are the template's own; only a list's count is words.
 */
export function variablePaths(t: TFor<"common">, value: unknown, prefix = ""): Variable[] {
  if (Array.isArray(value)) {
    const out: Variable[] = [
      { path: prefix, sample: t("common.items_count", { count: value.length }), isList: true },
    ];
    if (value.length > 0) out.push(...variablePaths(t, value[0], `${prefix}[0]`));
    return out;
  }
  if (typeof value === "object" && value !== null) {
    return Object.entries(value).flatMap(([key, inner]) =>
      variablePaths(t, inner, prefix ? `${prefix}.${key}` : key),
    );
  }
  const sample =
    value === null || value === undefined
      ? "null"
      : typeof value === "string"
        ? value
        : JSON.stringify(value);
  return [{ path: prefix, sample, isList: false }];
}

/** The Jinja that prints a path. */
export function printable(path: string): string {
  return `{{ ${path} }}`;
}

/** The longest a footer can be, as the API checks it. */
export const FOOTER_MAX = 1000;
/** The most staff recipients the API takes. */
export const STAFF_RECIPIENTS_MAX = 10;
