// Notifications in the admin: what each template is called and does, how a message's
// status reads, and the variables a template can print. Plain functions, for server and
// client code alike: the ones that build text take the caller's translator.

import { labelOr } from "./keys.ts";
import { sameTag } from "./languages.ts";
import type { TFor } from "./messages";

type NotificationsT = TFor<"notifications">;

/**
 * Whether a template is written per language: the buyer's emails, which go out in the
 * order's or the customer's language, and the layout they're framed in. Staff and
 * supplier emails are always in the store's content language, and an audience this build
 * doesn't know is treated like them.
 */
export function isTranslated(audience: string | null | undefined): boolean {
  return audience === "buyer" || audience === null || audience === undefined;
}

/**
 * The language a template page shows, from its `?language=`: one of `others` in its
 * canonical case, or `null` for the store's content language (asked for by name, not
 * asked, or a language the store doesn't have).
 */
export function templateLanguage(
  asked: string | undefined,
  others: readonly string[],
): string | null {
  if (asked === undefined) return null;
  return others.find((tag) => sameTag(tag, asked)) ?? null;
}

/** A template's editor, in `language` (`null`: the store's content language). */
export function templateHref(key: string, language: string | null): string {
  const path = `/settings/notifications/${encodeURIComponent(key)}`;
  return language ? `${path}?language=${encodeURIComponent(language)}` : path;
}

/** A template key as people read it; an unknown one as it is. */
export function templateName(t: NotificationsT, key: string): string {
  const name = `notifications.template_${key}_name`;
  return labelOr(t, name, key);
}

/** What a template is for, by its stable key; nothing for an unknown one. */
export function templateDescription(t: NotificationsT, key: string): string | undefined {
  const description = `notifications.template_${key}_description`;
  return labelOr(t, description, undefined);
}

const MESSAGE_STATUSES = ["queued", "sending", "sent", "failed", "skipped"] as const;

const MESSAGE_STATUS_VARIANTS: Record<
  (typeof MESSAGE_STATUSES)[number],
  "success" | "neutral" | "error" | "info"
> = {
  queued: "info",
  sending: "info",
  sent: "success",
  failed: "error",
  skipped: "neutral",
};

/**
 * A message's status as a badge: its label, in `t`'s language, and its colour. The order's
 * emails are where messages are listed, so the words are `orders`'. An unknown status as
 * it is.
 */
export function messageStatus(
  t: TFor<"orders">,
  status: string,
): { label: string; variant: "success" | "neutral" | "error" | "info" } {
  const known = MESSAGE_STATUSES.find((s) => s === status);
  if (!known) return { label: status, variant: "neutral" };
  return { label: t(`orders.emails_status_${known}`), variant: MESSAGE_STATUS_VARIANTS[known] };
}

/** Which source a template problem is in, as the editor names it; an unknown one as it is. */
function partName(t: NotificationsT, part: string): string {
  const name = `notifications.part_${part}`;
  return labelOr(t, name, part);
}

/** Where a template problem is: its part, its line (from 1, when known) and the words. */
export type Problem = { part: string; line?: number | null; message: string };

/** "HTML, line 4: …": a problem for a banner. The words are the API's, as it wrote them. */
export function describeProblem(t: NotificationsT, problem: Problem): string {
  const part = partName(t, problem.part);
  return problem.line
    ? t("notifications.problem_at_line", {
        part,
        line: String(problem.line),
        message: problem.message,
      })
    : t("notifications.problem_in_part", { part, message: problem.message });
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
