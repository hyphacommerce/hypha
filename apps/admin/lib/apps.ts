// How the admin labels apps (design notes 0016 §8, 0029): how each got here, whether it
// runs, and which of its scopes reach customers' personal data.

import type { AdminSchemas } from "@hypha/sdk/admin";

type App = AdminSchemas["AppResponse"];

export type AppState = {
  label: string;
  variant: "success" | "neutral" | "warning";
  description: string;
};

/** Where an app stands: installed through OAuth, made by hand, uninstalled, or off. */
export function appState(app: App): AppState {
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
    default:
      return scope;
  }
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
