// The sentence for an API error, by its stable `code`: `errors.<code>` in the catalogs.
// Plain functions over a translator, so they run on the server (`describe` in
// `lib/hypha.ts` passes the request's) and under `node --test`.

import type { ApiError } from "@hyphacommerce/sdk/admin";

import { hasKey } from "./keys.ts";
import type { TFor } from "./messages";
import { describeProblem, problemOf } from "./notifications.ts";
import { DEFAULT_LANGUAGE } from "./pick-language.ts";
import { PSEUDO } from "./pseudo.ts";

/** A number among an error's `details`, if the API sent one under `key`. */
function numberDetail(details: unknown, key: string): number | undefined {
  if (typeof details !== "object" || details === null || !(key in details)) return undefined;
  const value: unknown = Reflect.get(details, key);
  return typeof value === "number" ? value : undefined;
}

/** The API's sentence (`error.error`, English) as one of ours: "Too long" → "Too long". */
function sentence(text: string): string {
  return `${text.charAt(0).toUpperCase()}${text.slice(1)}`;
}

/**
 * Codes whose English sentence from the API says more than a fixed one could (which
 * field, which limit): shown as the API wrote it, in every language.
 */
const FROM_API: ReadonlySet<string> = new Set([
  "category_too_deep",
  "invalid_shipping_rate",
  "field_required",
  "field_too_long",
  "invalid_validation",
  "validation_required",
  "validation_not_allowed",
  "description_too_long",
  "agent_scope_not_grantable",
  "unknown_scope",
  "invalid_grant",
  "invalid_write_mode",
  "invalid_operation_id",
  "not_allowed_to_grant_apply",
]);

/**
 * A message for the person at the screen, by the API's stable error code, in `t`'s
 * language: `errors.<code>` in the catalog, with what the error's `details` add for the
 * few that carry some. A code the admin has no sentence for shows the API's own (English)
 * to an English reader, and a general sentence with the API's after it to anyone else.
 */
export function describeWith(t: TFor<"errors" | "notifications">, error: ApiError): string {
  switch (error.code) {
    case "payment_refused": {
      const reason =
        typeof error.details === "object" && error.details !== null
          ? Reflect.get(error.details, "reason")
          : undefined;
      return typeof reason === "string"
        ? t("errors.payment_refused_reason", { reason })
        : t("errors.payment_refused");
    }
    case "shipping_provider_failed":
    case "shipping_provider_timeout":
    case "invalid_payment_settings":
    case "email_unavailable":
    case "email_refused":
      return t(`errors.${error.code}`, { error: error.error });
    case "template_too_large":
      return t("errors.template_too_large", { error: sentence(error.error) });
    case "over_fulfilled": {
      const left = numberDetail(error.details, "unfulfilled");
      return left === undefined
        ? t("errors.over_fulfilled")
        : t("errors.over_fulfilled_left", { left });
    }
    case "template_invalid": {
      const problem = problemOf(error.details);
      return problem
        ? t("errors.template_invalid_problem", { problem: describeProblem(t, problem) })
        : t("errors.template_invalid");
    }
    case "custom_field_values_invalid": {
      const count = numberDetail(error.details, "count");
      return count === undefined
        ? t("errors.custom_field_values_invalid")
        : t("errors.custom_field_values_invalid_count", { count });
    }
    case "rate_limited": {
      const seconds = numberDetail(error.details, "retry_after_secs");
      return seconds ? t("errors.rate_limited_seconds", { seconds }) : t("errors.rate_limited");
    }
    default: {
      if (FROM_API.has(error.code)) return t("errors.from_api", { error: sentence(error.error) });
      const key = `errors.${error.code}`;
      // Every sentence may use the API's own words; most don't.
      if (hasKey(t, key)) return t(key, { error: error.error });
      // The API's own sentence is English: as it is for an English reader, and behind a
      // sentence of the reader's language for anyone else, so the cause is still there.
      return t.language === DEFAULT_LANGUAGE || t.language === PSEUDO
        ? error.error
        : t("errors.unknown", { error: error.error });
    }
  }
}
