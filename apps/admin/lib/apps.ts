// How the admin labels apps (design notes 0016 §8, 0029): how each got here, whether it
// runs, and which of its scopes reach customers' personal data.

import type { AdminSchemas } from "@hypha/sdk/admin";

type App = AdminSchemas["AppResponse"];

export type AppState = {
  label: string;
  variant: "success" | "neutral" | "warning";
  description: string;
};

/** Whether an app is a spore: built into the API rather than a service (design note 0030). */
export function isBuiltIn(app: Pick<App, "kind">): boolean {
  return app.kind === "spore";
}

/** Where an app stands: installed through OAuth, made by hand, built in, uninstalled, or off. */
export function appState(app: App & { manifest?: { url: string } | null }): AppState {
  if (isBuiltIn(app) && app.missing) {
    return {
      label: "Not built in",
      variant: "warning",
      description:
        "Taken out of this API's build: its settings and data stay, and it does nothing until a build has it again.",
    };
  }
  if (isBuiltIn(app)) {
    return app.enabled
      ? {
          // The "Built in" badge sits beside the name; the status says whether it runs.
          label: "On",
          variant: "success",
          description: "Built into this store's API by its operator.",
        }
      : {
          label: "Off",
          variant: "neutral",
          description: "Built in, and turned off: it collects nothing and adds nothing.",
        };
  }
  if (app.uninstalled_at) {
    return {
      label: "Uninstalled",
      variant: "warning",
      description: "Its access ended; it only got the app.uninstalled event.",
    };
  }
  if (!app.enabled) {
    return {
      label: "Off",
      variant: "neutral",
      description: "It gets no webhooks and quotes no rates.",
    };
  }
  if (app.installed && app.manifest?.url.startsWith("urn:hypha:private:")) {
    return {
      label: "Private",
      variant: "success",
      description: "A private app: its manifest was uploaded to this store.",
    };
  }
  if (app.installed) {
    return { label: "Installed", variant: "success", description: "Installed through OAuth." };
  }
  return {
    label: "Custom",
    variant: "success",
    description: "Made by hand under Settings → Developers.",
  };
}

/** Whether a scope unlocks customers' personal data (`customer_data:*`, AP10). */
export function isPersonalDataScope(scope: string): boolean {
  return scope.startsWith("customer_data:");
}

/** What a personal-data scope lets an app see, for people rather than code. */
export function personalDataLabel(scope: string): string {
  switch (scope) {
    case "customer_data:name":
      return "Customers' names";
    case "customer_data:email":
      return "Customers' email addresses";
    case "customer_data:phone":
      return "Customers' phone numbers";
    case "customer_data:address":
      return "Customers' addresses";
    case "customer_data:profile":
      return "Customers' personal custom fields";
    default:
      return scope;
  }
}

/** An area of scopes (`orders`), as people say it. */
const AREAS: Record<string, string> = {
  apps: "apps",
  carts: "carts",
  catalogs: "catalogs and price lists",
  categories: "categories",
  channels: "sales channels",
  checkout: "checkout",
  custom_fields: "custom fields",
  customers: "customers",
  inventory: "inventory",
  keys: "API keys",
  markets: "markets",
  media: "media files",
  notifications: "emails",
  orders: "orders",
  payments: "payments",
  pricing: "prices",
  products: "products",
  search: "search",
  shipping: "shipping",
  staff: "staff",
  stores: "store details",
  tax: "taxes",
  taxes: "taxes",
  users: "staff",
};

/** An area's name for a heading: "Orders", "Sales channels". */
export function areaLabel(area: string): string {
  const name = AREAS[area] ?? area.replaceAll("_", " ");
  return name.charAt(0).toUpperCase() + name.slice(1);
}

/** What a scope lets an app do, in plain words: "See orders", "Change products". */
export function scopeLabel(scope: string): string {
  if (isPersonalDataScope(scope)) return personalDataLabel(scope);
  const [area = scope, action = ""] = scope.split(":");
  const what = AREAS[area] ?? area.replaceAll("_", " ");
  if (action === "read") return `See ${what}`;
  if (action === "write") return `See and change ${what}`;
  return `${action.charAt(0).toUpperCase()}${action.slice(1)} ${what}`;
}

/** Who did something, for a line of history: "a staff member", "the app itself", … */
export function actorLabel(actor: AdminSchemas["EventActor"] | null | undefined): string {
  switch (actor?.type) {
    case "user":
      return "a staff member";
    case "agent":
      return "an agent";
    case "api_key":
      return "an API key";
    case "app":
      return "the app itself";
    case "system":
      return "Hypha";
    default:
      return "someone";
  }
}
