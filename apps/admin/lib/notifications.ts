// Notifications in the admin (design note 0028): what each template is called and does,
// how a message's status reads, and the variables a template can print. Plain data, for
// server and client code alike.

/** A template's name and what it's for, by its stable key. */
export const TEMPLATES: Record<string, { name: string; description: string }> = {
  layout: {
    name: "Email layout",
    description: "The shared frame around every email: logo, colours and footer.",
  },
  order_confirmation: {
    name: "Order confirmation",
    description: "Sent to the customer when they place an order.",
  },
  order_shipped: {
    name: "Order shipped",
    description: "Sent when you fulfil items and choose to notify the customer.",
  },
  order_tracking_updated: {
    name: "Tracking updated",
    description: "Sent when a shipment's tracking changes and you choose to notify.",
  },
  order_cancelled: {
    name: "Order cancelled",
    description: "Sent when you cancel an order and choose to notify the customer.",
  },
  staff_new_order: {
    name: "New order",
    description: "Sent to the staff recipients when an order is placed.",
  },
  staff_invite: {
    name: "Staff invite",
    description: "Sent to someone invited to the admin, with the link to join.",
  },
  staff_password_reset: {
    name: "Password reset",
    description: "Sent to a staff member who asked to reset their password.",
  },
  customer_login_code: {
    name: "Customer sign-in code",
    description: "Sent to a customer signing in to the storefront, with their six-digit code.",
  },
  customer_erasure_requested: {
    name: "Erasure requested",
    description:
      "Sent to a customer who asked from their account to have their data erased, with the date it runs.",
  },
};

/** A template key as people read it; an unknown one as it is. */
export function templateName(key: string): string {
  return TEMPLATES[key]?.name ?? key;
}

/** A message's status: its label and badge colour. */
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

/** Which source a template problem is in, as the editor names it. */
export const PART_NAMES: Record<string, string> = {
  subject: "Subject",
  html: "HTML",
  text: "Plain text",
  layout: "Email layout",
};

/** Where a template problem is: its part, its line (from 1, when known) and the words. */
export type Problem = { part: string; line?: number | null; message: string };

/** "HTML, line 4: …": a problem for a banner. */
export function describeProblem(problem: Problem): string {
  const part = PART_NAMES[problem.part] ?? problem.part;
  return problem.line
    ? `${part}, line ${problem.line}: ${problem.message}`
    : `${part}: ${problem.message}`;
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
 * variable (`{% for line in order.lines %}{{ line.title }}`) is easy to find.
 */
export function variablePaths(value: unknown, prefix = ""): Variable[] {
  if (Array.isArray(value)) {
    const out: Variable[] = [{ path: prefix, sample: `${value.length} items`, isList: true }];
    if (value.length > 0) out.push(...variablePaths(value[0], `${prefix}[0]`));
    return out;
  }
  if (typeof value === "object" && value !== null) {
    return Object.entries(value).flatMap(([key, inner]) =>
      variablePaths(inner, prefix ? `${prefix}.${key}` : key),
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
