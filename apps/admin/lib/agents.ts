// How the admin shows agents, their grants and what they propose: which scopes a grant
// can name, labels for statuses and operations, and the preview a proposal carries, read
// from the API's untyped JSON into typed values. Shared by server and client components.

import type { AdminSchemas } from "@hyphacommerce/sdk/admin";

import { areaLabel, personalDataLabel } from "./apps";

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
];

/** Write scopes an agent may only propose: a person approves every use. */
export const PROPOSE_ONLY = ["customers:erase"];

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
  settings: Pick<AgentSettings, "enabled" | "writes">,
): { title: string; description: string } | null {
  if (!settings.enabled) {
    return {
      title: "Agents are turned off on this deployment",
      description:
        "The operator set agents.enabled to false: no agent can connect or act, whatever its grant. Grants and proposals are kept for when they're turned on.",
    };
  }
  if (settings.writes === "propose") {
    return {
      title: "Agents only propose in this deployment",
      description:
        'The operator set agents.writes to "propose": every write an agent makes waits for a person to approve it, whatever its grant says.',
    };
  }
  if (settings.writes === "off") {
    return {
      title: "Agents only read in this deployment",
      description:
        'The operator set agents.writes to "off": agents can\'t write, preview or propose. Grants keep their write scopes for when the cap is lifted.',
    };
  }
  return null;
}

/** An agent's state, for its badge. */
export function agentState(agent: Agent): {
  label: string;
  variant: "success" | "neutral" | "error";
} {
  if (agent.revoked_at) return { label: "Revoked", variant: "error" };
  return agent.enabled
    ? { label: "Enabled", variant: "success" }
    : { label: "Disabled", variant: "neutral" };
}

/** "Connected" (acts for a staff member) or "Autonomous" (its own key). */
export function kindLabel(kind: string): string {
  return kind === "connected" ? "Connected" : kind === "autonomous" ? "Autonomous" : kind;
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

const PROPOSAL_STATUS: Record<string, { label: string; variant: Variant }> = {
  pending: { label: "Waiting", variant: "warning" },
  applying: { label: "Applying", variant: "info" },
  applied: { label: "Applied", variant: "success" },
  rejected: { label: "Rejected", variant: "neutral" },
  expired: { label: "Expired", variant: "neutral" },
  failed: { label: "Failed", variant: "error" },
};

/** A proposal's status; an unknown one shows as it is. */
export function proposalStatus(status: string): { label: string; variant: Variant } {
  return PROPOSAL_STATUS[status] ?? { label: status, variant: "neutral" };
}

const MODES: Record<string, string> = {
  read: "Read",
  preview: "Preview",
  propose: "Propose",
  apply: "Apply",
};

/** A call's mode in the activity log. */
export function modeLabel(mode: string): string {
  return MODES[mode] ?? mode;
}

/**
 * An operation in words: the first sentence of its summary from the API ("Changes a
 * product's title, handle…" stops at the first colon or full stop), else words from its
 * ID ("Products: update variant") for an operation this build no longer has.
 */
export function operationLabel(operation: string, summary?: string | null): string {
  const first = summary
    ?.split(/[.:](?:\s|$)/)[0]
    ?.replace(/\s+/g, " ")
    .trim();
  if (first) return first;
  const [, area = operation, action = ""] = operation.split(".");
  const words = action.replaceAll("_", " ");
  return action ? `${areaLabel(area)}: ${words}` : areaLabel(area);
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

/** A prefixed ID shortened for reading: `prod_…a1b2c3`. */
export function shortId(id: string): string {
  const cut = id.indexOf("_");
  return cut > 0 && id.length > cut + 10 ? `${id.slice(0, cut + 1)}…${id.slice(-6)}` : id;
}

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
export function blockedLabel(blocked: NonNullable<Preview["blocked_by"]>): string {
  switch (blocked.kind) {
    case "extension_call":
      return `It stops where it would call ${blocked.app} (${blocked.point}), which a preview can't do without the effect happening.`;
    case "outbound_call":
      return "It stops where it would call out to another service, which a preview can't do.";
    case "media_storage":
      return "It stops where it would touch the media storage, which a preview can't do.";
    case "media_upload":
      return "It stops where it would hand out an upload link, which a preview can't do.";
    case "proposal":
      return "It stops where it would apply another proposal.";
    default:
      return "It stops at an effect a preview can't run.";
  }
}

// ---------- events as sentences ----------

/** A piece of a sentence: words, or a record (linked when it has a page). */
export type Part = string | { id: string };

type Data = Record<string, unknown>;

const id = (value: unknown): Part => ({ id: text(value) ?? "?" });

function fields(data: Data): string {
  const list = texts(data.fields).map((f) => f.replaceAll("_", " "));
  return list.length ? list.join(", ") : "its details";
}

function count(value: unknown, one: string, many: string): string {
  const n = Array.isArray(value) ? value.length : 0;
  return `${n} ${n === 1 ? one : many}`;
}

/**
 * A sentence for the common event types, as parts the page renders (IDs as links);
 * `null` for any other, which the page shows as its JSON.
 */
export function eventSentence(type: string, data: Data): Part[] | null {
  switch (type) {
    case "product.created":
      return [
        "Creates product ",
        id(data.product),
        ` (${text(data.handle) ?? ""}, ${text(data.status) ?? "draft"})`,
      ];
    case "product.updated":
      return ["Changes product ", id(data.product), `: ${fields(data)}`];
    case "product.deleted":
      return [
        "Deletes product ",
        id(data.product),
        ` with ${count(data.variants, "variant", "variants")}`,
      ];
    case "product.variant_added":
      return ["Adds variant ", id(data.variant), " to product ", id(data.product)];
    case "product.variant_updated":
      return [
        "Changes variant ",
        id(data.variant),
        " of product ",
        id(data.product),
        `: ${fields(data)}`,
      ];
    case "product.variant_removed":
      return ["Removes variant ", id(data.variant), " from product ", id(data.product)];
    case "inventory_item.changed": {
      const levels = Array.isArray(data.available) ? data.available.filter(isRecord) : [];
      const parts: Part[] = ["Stock of variant ", id(data.variant), " becomes "];
      if (levels.length === 0) parts.push("nothing anywhere");
      levels.forEach((level, i) => {
        if (i > 0) parts.push(", ");
        parts.push(`${typeof level.available === "number" ? level.available : "?"} available at `);
        parts.push(id(level.location));
      });
      if (data.tracked === false) parts.push(" (not tracked)");
      return parts;
    }
    case "inventory_item.deleted":
      return ["Stops tracking stock of variant ", id(data.variant)];
    case "location.changed":
      return ["Changes location ", id(data.location), `: ${fields(data)}`];
    case "price_list.prices_set":
      return [
        "Sets fixed prices for ",
        count(data.variants, "variant", "variants"),
        " in price list ",
        id(data.price_list),
      ];
    case "price_list.prices_removed":
      return [
        "Removes fixed prices of ",
        count(data.variants, "variant", "variants"),
        " from price list ",
        id(data.price_list),
      ];
    case "price_list.changed":
      return ["Changes price list ", id(data.price_list), "'s adjustment"];
    case "catalog.products_added":
      return [
        "Adds ",
        count(data.products, "product", "products"),
        " to catalog ",
        id(data.catalog),
      ];
    case "catalog.products_removed":
      return [
        "Removes ",
        count(data.products, "product", "products"),
        " from catalog ",
        id(data.catalog),
      ];
    case "order.cancelled":
      return [
        "Cancels order ",
        id(data.order),
        ` (reason: ${text(data.reason) ?? "other"}${data.notify_customer === true ? "; emails the buyer" : ""})`,
      ];
    case "order.paid":
      return ["Marks order ", id(data.order), " paid"];
    case "order.fulfillment_status_changed":
      return [
        "Order ",
        id(data.order),
        ` goes from ${(text(data.from) ?? "").replaceAll("_", " ")} to ${(text(data.to) ?? "").replaceAll("_", " ")}`,
      ];
    case "fulfillment.created": {
      const lines = Array.isArray(data.lines) ? data.lines.filter(isRecord) : [];
      const units = lines.reduce(
        (n, l) => n + (typeof l.quantity === "number" ? l.quantity : 0),
        0,
      );
      return [
        "Ships ",
        `${units} ${units === 1 ? "unit" : "units"} of order `,
        id(data.order),
        " from ",
        id(data.location),
        data.notify_customer === true ? " and emails the buyer" : "",
      ];
    }
    case "fulfillment.updated":
      return ["Changes the tracking of a shipment of order ", id(data.order)];
    case "fulfillment.cancelled":
      return ["Takes back a shipment of order ", id(data.order), "; its units return to stock"];
    case "category.changed":
      return data.created === true
        ? ["Creates category ", id(data.category)]
        : ["Changes category ", id(data.category), `: ${fields(data)}`];
    case "category.deleted":
      return ["Deletes category ", id(data.category)];
    case "category.products_added":
      return [
        "Adds ",
        count(data.products, "product", "products"),
        " to category ",
        id(data.category),
      ];
    case "category.products_removed":
      return [
        "Removes ",
        count(data.products, "product", "products"),
        " from category ",
        id(data.category),
      ];
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
