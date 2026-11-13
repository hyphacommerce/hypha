// How the admin shows agents, their grants and what they propose: which scopes a grant
// can name, labels for statuses and operations, and the preview a proposal carries, read
// from the API's untyped JSON into typed values. Shared by server and client components,
// so each function that writes words takes the caller's translator.

import type { AdminSchemas } from "@hyphacommerce/sdk/admin";

import { areaLabel, personalDataLabel } from "./apps";
import type { Format } from "./format";
import { labelOr } from "./keys";
import type { KeyIn, TFor } from "./messages";

/** A translator with the agents' messages, the apps' (areas, scopes) and the vocabulary. */
type Translate = TFor<"agents" | "apps" | "common">;

export type Agent = AdminSchemas["AgentResponse"];
export type Proposal = AdminSchemas["ProposalResponse"];
/**
 * A write's preview (`PreviewBody`), with its bodies and event data as JSON records: the
 * SDK types them as empty objects, since the API doesn't fix their shape.
 */
export type Preview = Omit<AdminSchemas["PreviewBody"], "response" | "events"> & {
  response: { status: number; body: Record<string, unknown> } | null;
  events: PreviewEvent[];
};

export type PreviewEvent = {
  type: string;
  version: number;
  subject: string;
  data: Record<string, unknown>;
};
export type WriteMode = "propose" | "apply";
export type WritesCap = "apply" | "propose" | "off";

/**
 * The areas a grant names, in the API's scope order, each with a read scope. The ones
 * whose write scope no agent is ever granted (it would widen its own reach or mint
 * credentials: staff, keys, apps, agents) offer reading only. `checkout` is never
 * granted at all, so it isn't listed.
 */
export const GRANT_AREAS: { area: string; canWrite: boolean }[] = [
  { area: "products", canWrite: true },
  { area: "markets", canWrite: true },
  { area: "pricing", canWrite: true },
  { area: "carts", canWrite: true },
  { area: "taxes", canWrite: true },
  { area: "inventory", canWrite: true },
  { area: "customers", canWrite: true },
  { area: "stores", canWrite: true },
  { area: "orders", canWrite: true },
  { area: "users", canWrite: false },
  { area: "api_keys", canWrite: false },
  { area: "media", canWrite: true },
  { area: "shipping", canWrite: true },
  { area: "apps", canWrite: false },
  { area: "fulfillments", canWrite: true },
  { area: "channels", canWrite: true },
  { area: "payments", canWrite: true },
  { area: "notifications", canWrite: true },
  { area: "custom_fields", canWrite: true },
  { area: "agents", canWrite: false },
  { area: "discounts", canWrite: true },
  { area: "purchasing", canWrite: true },
  { area: "checkout_rules", canWrite: true },
  { area: "translations", canWrite: true },
];

/** Write scopes an agent may only propose (a person approves every use), with their labels. */
export const PROPOSE_ONLY: { scope: string; label: KeyIn<"agents"> }[] = [
  { scope: "customers:erase", label: "agents.propose_only_customers_erase" },
  { scope: "purchasing:send", label: "agents.propose_only_purchasing_send" },
];

/** The personal data a grant may let an agent see; none by default. */
export const CUSTOMER_DATA = [
  "customer_data:name",
  "customer_data:email",
  "customer_data:phone",
  "customer_data:address",
  "customer_data:profile",
];

export { areaLabel, personalDataLabel };

export type AgentSettings = AdminSchemas["AgentSettingsResponse"];

/**
 * What the operator's `[agents]` settings mean, for a banner: agents turned off, or the
 * cap on what they write; `null` when grants decide.
 */
export function capNotice(
  t: TFor<"agents">,
  settings: Pick<AgentSettings, "enabled" | "writes">,
): { title: string; description: string } | null {
  if (!settings.enabled) {
    return {
      title: t("agents.cap_disabled_title"),
      description: t("agents.cap_disabled_description", {
        setting: "agents.enabled",
        value: "false",
      }),
    };
  }
  if (settings.writes === "propose") {
    return {
      title: t("agents.cap_propose_title"),
      description: t("agents.cap_propose_description", {
        setting: "agents.writes",
        value: "propose",
      }),
    };
  }
  if (settings.writes === "off") {
    return {
      title: t("agents.cap_off_title"),
      description: t("agents.cap_off_description", { setting: "agents.writes", value: "off" }),
    };
  }
  return null;
}

/** An agent's state, for its badge. */
export function agentState(
  t: Translate,
  agent: Agent,
): {
  label: string;
  variant: "success" | "neutral" | "error";
} {
  if (agent.revoked_at) return { label: t("agents.state_revoked"), variant: "error" };
  return agent.enabled
    ? { label: t("common.enabled"), variant: "success" }
    : { label: t("common.disabled"), variant: "neutral" };
}

/** "Connected" (acts for a staff member) or "Autonomous" (its own key); another kind as it is. */
export function kindLabel(t: TFor<"agents">, kind: string): string {
  const key = `agents.kind_${kind}`;
  return labelOr(t, key, kind);
}

type Variant = "success" | "neutral" | "warning" | "info" | "error";

/** Every status a proposal has, as the list filters them. */
export const PROPOSAL_STATUSES = [
  "pending",
  "applying",
  "applied",
  "rejected",
  "expired",
  "failed",
] as const;

const PROPOSAL_VARIANT: Record<string, Variant> = {
  pending: "warning",
  applying: "info",
  applied: "success",
  rejected: "neutral",
  expired: "neutral",
  failed: "error",
};

/** A proposal's status; an unknown one shows as it is. */
export function proposalStatus(
  t: TFor<"agents">,
  status: string,
): { label: string; variant: Variant } {
  const key = `agents.proposal_status_${status}`;
  return {
    label: labelOr(t, key, status),
    variant: PROPOSAL_VARIANT[status] ?? "neutral",
  };
}

/** A call's mode in the activity log; an unknown one shows as it is. */
export function modeLabel(t: TFor<"agents">, mode: string): string {
  const key = `agents.mode_${mode}`;
  return labelOr(t, key, mode);
}

/**
 * An operation in words: the first sentence of its summary from the API ("Changes a
 * product's title, handle…" stops at the first colon or full stop; the API's own English),
 * else words from its ID ("Products: update variant") for an operation this build no
 * longer has.
 */
export function operationLabel(t: Translate, operation: string, summary?: string | null): string {
  const first = summary
    ?.split(/[.:](?:\s|$)/)[0]
    ?.replace(/\s+/g, " ")
    .trim();
  if (first) return first;
  const [, area = operation, action = ""] = operation.split(".");
  const words = action.replaceAll("_", " ");
  return action
    ? t("agents.operation_in_area", { area: areaLabel(t, area), action: words })
    : areaLabel(t, area);
}

/** Where a prefixed ID's record is in the admin, for the kinds that have a page. */
export function recordHref(id: string): string | undefined {
  const prefix = id.slice(0, id.indexOf("_"));
  switch (prefix) {
    case "prod":
      return `/products/${id}`;
    case "ord":
      return `/orders/${id}`;
    case "cus":
      return `/customers/${id}`;
    case "pcat":
      return `/products/categories/${id}`;
    case "mkt":
      return `/markets/${id}`;
    case "asset":
      return `/media/${id}`;
    case "agent":
      return `/agents/${id}`;
    default:
      return undefined;
  }
}

export { shortId } from "./ids";

// ---------- the API's untyped JSON, read into types ----------

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function text(value: unknown): string | undefined {
  return typeof value === "string" ? value : undefined;
}

function texts(value: unknown): string[] {
  return Array.isArray(value) ? value.filter((v): v is string => typeof v === "string") : [];
}

function blockedBy(value: unknown): Preview["blocked_by"] {
  if (!isRecord(value)) return null;
  const kind = text(value.kind);
  switch (kind) {
    case "extension_call":
      return { kind, point: text(value.point) ?? "", app: text(value.app) ?? "" };
    case "outbound_call":
    case "media_storage":
    case "media_upload":
    case "proposal":
      return { kind };
    default:
      return null;
  }
}

/**
 * A preview (`PreviewBody`) from JSON the API typed as an object (a proposal's, or a
 * `preview_stale`'s `details.preview`); `null` when it isn't one.
 */
export function readPreview(value: unknown): Preview | null {
  if (!isRecord(value) || typeof value.complete !== "boolean") return null;
  const events = Array.isArray(value.events) ? value.events : [];
  const emails = Array.isArray(value.emails) ? value.emails : [];
  const calls = Array.isArray(value.calls) ? value.calls : [];
  const response = isRecord(value.response) ? value.response : undefined;
  return {
    complete: value.complete,
    blocked_by: blockedBy(value.blocked_by),
    response:
      response && typeof response.status === "number"
        ? { status: response.status, body: isRecord(response.body) ? response.body : {} }
        : null,
    events: events.filter(isRecord).map((e) => ({
      type: text(e.type) ?? "",
      version: typeof e.version === "number" ? e.version : 1,
      subject: text(e.subject) ?? "",
      data: isRecord(e.data) ? e.data : {},
    })),
    emails: emails.filter(isRecord).map((e) => ({
      template: text(e.template) ?? "",
      subject: text(e.subject) ?? "",
      to: text(e.to) ?? null,
      redacted: texts(e.redacted),
    })),
    calls: calls.filter(isRecord).map((c) => ({
      point: text(c.point) ?? "",
      app: text(c.app) ?? "",
    })),
  };
}

/** What blocked a preview, in words. */
export function blockedLabel(
  t: TFor<"agents">,
  blocked: NonNullable<Preview["blocked_by"]>,
): string {
  switch (blocked.kind) {
    case "extension_call":
      return t("agents.blocked_extension_call", { app: blocked.app, point: blocked.point });
    case "outbound_call":
      return t("agents.blocked_outbound_call");
    case "media_storage":
      return t("agents.blocked_media_storage");
    case "media_upload":
      return t("agents.blocked_media_upload");
    case "proposal":
      return t("agents.blocked_proposal");
    default:
      return t("agents.blocked_unknown");
  }
}

// ---------- events as sentences ----------

/** A piece of a sentence: words, or a record (linked when it has a page). */
export type Part = string | { id: string };

type Data = Record<string, unknown>;

const id = (value: unknown): { id: string } => ({ id: text(value) ?? "?" });

/** How many items a list in an event's data has. */
const size = (value: unknown): number => (Array.isArray(value) ? value.length : 0);

/** An identifier as words: `partially_fulfilled` → "partially fulfilled". */
const words = (value: unknown): string => (text(value) ?? "").replaceAll("_", " ");

/**
 * A sentence for the common event types, as parts the page renders (IDs as links);
 * `null` for any other, which the page shows as its JSON. Field names, statuses and
 * reasons are the API's identifiers, shown as they are.
 */
export function eventSentence(
  t: TFor<"agents">,
  format: Format,
  type: string,
  data: Data,
): Part[] | null {
  const rich = (key: KeyIn<"agents">, values: Record<string, string | number | { id: string }>) =>
    t.rich<{ id: string }>(key, values);
  /** What changed, for a message's `{count}` and `{fields}`: none reads "its details". */
  const changed = () => {
    const list = texts(data.fields).map((f) => f.replaceAll("_", " "));
    return { count: list.length, fields: format.list(list, "unit") };
  };
  switch (type) {
    case "product.created":
      return rich("agents.event_product_created", {
        product: id(data.product),
        handle: text(data.handle) ?? "",
        status: text(data.status) ?? "draft",
      });
    case "product.updated":
      return rich("agents.event_product_updated", { product: id(data.product), ...changed() });
    case "product.deleted":
      return rich("agents.event_product_deleted", {
        product: id(data.product),
        count: size(data.variants),
      });
    case "product.variant_added":
      return rich("agents.event_variant_added", {
        variant: id(data.variant),
        product: id(data.product),
      });
    case "product.variant_updated":
      return rich("agents.event_variant_updated", {
        variant: id(data.variant),
        product: id(data.product),
        ...changed(),
      });
    case "product.variant_removed":
      return rich("agents.event_variant_removed", {
        variant: id(data.variant),
        product: id(data.product),
      });
    case "inventory_item.changed": {
      const levels = Array.isArray(data.available) ? data.available.filter(isRecord) : [];
      const untracked = data.tracked === false;
      if (levels.length === 0) {
        return rich(
          untracked ? "agents.event_stock_nothing_untracked" : "agents.event_stock_nothing",
          { variant: id(data.variant) },
        );
      }
      // The levels are a list of their own inside the sentence: `{levels}` marks its place.
      const place = { id: "" };
      const list = levels.flatMap((level, i): Part[] => [
        ...(i > 0 ? [", "] : []),
        ...rich("agents.event_stock_level", {
          available: typeof level.available === "number" ? level.available : "?",
          location: id(level.location),
        }),
      ]);
      return rich(untracked ? "agents.event_stock_levels_untracked" : "agents.event_stock_levels", {
        variant: id(data.variant),
        levels: place,
      }).flatMap((part) => (part === place ? list : [part]));
    }
    case "inventory_item.deleted":
      return rich("agents.event_stock_deleted", { variant: id(data.variant) });
    case "location.changed":
      return rich("agents.event_location_changed", { location: id(data.location), ...changed() });
    case "price_list.prices_set":
      return rich("agents.event_prices_set", {
        count: size(data.variants),
        price_list: id(data.price_list),
      });
    case "price_list.prices_removed":
      return rich("agents.event_prices_removed", {
        count: size(data.variants),
        price_list: id(data.price_list),
      });
    case "price_list.created":
      return rich("agents.event_price_list_created", { price_list: id(data.price_list) });
    case "price_list.changed":
      return rich("agents.event_price_list_changed", {
        price_list: id(data.price_list),
        ...changed(),
      });
    case "catalog.products_added":
      return rich("agents.event_catalog_products_added", {
        count: size(data.products),
        catalog: id(data.catalog),
      });
    case "catalog.products_removed":
      return rich("agents.event_catalog_products_removed", {
        count: size(data.products),
        catalog: id(data.catalog),
      });
    case "price_list.deleted":
      return rich("agents.event_price_list_deleted", { price_list: id(data.price_list) });
    case "catalog.price_lists_added":
      return rich("agents.event_catalog_price_lists_added", {
        count: size(data.price_lists),
        catalog: id(data.catalog),
      });
    case "catalog.price_lists_removed":
      return rich("agents.event_catalog_price_lists_removed", {
        count: size(data.price_lists),
        catalog: id(data.catalog),
      });
    case "order.cancelled":
      return rich(
        data.notify_customer === true
          ? "agents.event_order_cancelled_notify"
          : "agents.event_order_cancelled",
        { order: id(data.order), reason: text(data.reason) ?? "other" },
      );
    case "order.paid":
      return rich("agents.event_order_paid", { order: id(data.order) });
    case "order.fulfillment_status_changed":
      return rich("agents.event_order_fulfillment_status", {
        order: id(data.order),
        from: words(data.from),
        to: words(data.to),
      });
    case "fulfillment.created": {
      const lines = Array.isArray(data.lines) ? data.lines.filter(isRecord) : [];
      const units = lines.reduce(
        (n, line) => n + (typeof line.quantity === "number" ? line.quantity : 0),
        0,
      );
      return rich(
        data.notify_customer === true
          ? "agents.event_fulfillment_created_notify"
          : "agents.event_fulfillment_created",
        { count: units, order: id(data.order), location: id(data.location) },
      );
    }
    case "fulfillment.updated":
      return rich("agents.event_fulfillment_updated", { order: id(data.order) });
    case "fulfillment.cancelled":
      return rich("agents.event_fulfillment_cancelled", { order: id(data.order) });
    case "category.changed":
      return data.created === true
        ? rich("agents.event_category_created", { category: id(data.category) })
        : rich("agents.event_category_changed", { category: id(data.category), ...changed() });
    case "category.deleted":
      return rich("agents.event_category_deleted", { category: id(data.category) });
    case "category.products_added":
      return rich("agents.event_category_products_added", {
        count: size(data.products),
        category: id(data.category),
      });
    case "category.products_removed":
      return rich("agents.event_category_products_removed", {
        count: size(data.products),
        category: id(data.category),
      });
    default:
      return null;
  }
}

/**
 * Names for the records a preview mentions, from its response body (a product's title,
 * a category's name) and what the page knows (locations), by ID.
 */
export function previewNames(
  preview: Preview,
  known: Record<string, string>,
): Record<string, string> {
  const names = { ...known };
  const body = preview.response?.body;
  if (isRecord(body)) {
    const bodyId = text(body.id);
    const label = text(body.title) ?? text(body.name);
    if (bodyId && label) names[bodyId] = label;
  }
  return names;
}
