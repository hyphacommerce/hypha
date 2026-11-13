// How the admin labels apps: how each got here, whether it runs, and which of its
// scopes reach customers' personal data. Shared by server and client components, so
// each function that writes words takes the caller's translator.

import type { AdminSchemas } from "@hyphacommerce/sdk/admin";

import { hasKey } from "./keys";
import type { TFor } from "./messages";

type App = AdminSchemas["AppResponse"];

/** A translator with the apps' messages and the shared vocabulary. */
type Translate = TFor<"apps" | "common">;

export type AppState = {
  label: string;
  variant: "success" | "neutral" | "warning";
  description: string;
};

/** Whether an app is a spore: built into the API rather than a service. */
export function isBuiltIn(app: Pick<App, "kind">): boolean {
  return app.kind === "spore";
}

/** Where an app stands: installed through OAuth, made by hand, built in, uninstalled, or off. */
export function appState(t: Translate, app: App & { manifest?: { url: string } | null }): AppState {
  if (isBuiltIn(app) && app.missing) {
    return {
      label: t("apps.state_not_built_in"),
      variant: "warning",
      description: t("apps.state_not_built_in_description"),
    };
  }
  if (isBuiltIn(app)) {
    return app.enabled
      ? {
          // The "Built in" badge sits beside the name; the status says whether it runs.
          label: t("common.on"),
          variant: "success",
          description: t("apps.state_built_in_on_description"),
        }
      : {
          label: t("common.off"),
          variant: "neutral",
          description: t("apps.state_built_in_off_description"),
        };
  }
  if (app.uninstalled_at) {
    return {
      label: t("apps.state_uninstalled"),
      variant: "warning",
      description: t("apps.state_uninstalled_description", { event: "app.uninstalled" }),
    };
  }
  if (!app.enabled) {
    return {
      label: t("common.off"),
      variant: "neutral",
      description: t("apps.state_off_description"),
    };
  }
  if (app.installed && app.manifest?.url.startsWith("urn:hypha:private:")) {
    return {
      label: t("apps.state_private"),
      variant: "success",
      description: t("apps.state_private_description"),
    };
  }
  if (app.installed) {
    return {
      label: t("apps.state_installed"),
      variant: "success",
      description: t("apps.state_installed_description"),
    };
  }
  return {
    label: t("apps.state_custom"),
    variant: "success",
    description: t("apps.state_custom_description"),
  };
}

/** Whether a scope unlocks customers' personal data (`customer_data:*`, AP10). */
export function isPersonalDataScope(scope: string): boolean {
  return scope.startsWith("customer_data:");
}

/**
 * What a personal-data scope lets an app see, for people rather than code; a scope the
 * admin has no words for shows as it is.
 */
export function personalDataLabel(t: TFor<"apps">, scope: string): string {
  const key = `apps.personal_data_${scope.slice(scope.indexOf(":") + 1)}`;
  return isPersonalDataScope(scope) && hasKey(t, key) ? t(key) : scope;
}

/** Older names of areas, still met in operation IDs: each reads as the area it became. */
const AREA_ALIASES: Record<string, string> = { keys: "api_keys", staff: "users", tax: "taxes" };

function canonicalArea(area: string): string {
  return AREA_ALIASES[area] ?? area;
}

/** An area the admin has no name for, from its identifier: "price_lists" → "price lists". */
function areaWords(area: string): string {
  return area.replaceAll("_", " ");
}

/** An area of scopes (`orders`) for a heading, as people say it: "Orders", "Sales channels". */
export function areaLabel(t: TFor<"apps">, area: string): string {
  const key = `apps.area_${canonicalArea(area)}`;
  if (hasKey(t, key)) return t(key);
  const words = areaWords(area);
  return words.charAt(0).toUpperCase() + words.slice(1);
}

/**
 * What a scope lets an app do, in plain words: "See orders", "See and change products".
 * A scope the admin has no sentence for shows as it is.
 */
export function scopeLabel(t: TFor<"apps">, scope: string): string {
  if (isPersonalDataScope(scope)) return personalDataLabel(t, scope);
  const [area = scope, action = ""] = scope.split(":");
  const canonical = canonicalArea(area);
  // The scopes that aren't a plain read or write of their area: `customers:erase`,
  // `purchasing:send`, `checkout`.
  const own = action ? `apps.scope_${canonical}_${action}` : `apps.scope_${canonical}`;
  if (action === "read") {
    const key = `apps.scope_read_${canonical}`;
    return hasKey(t, key) ? t(key) : t("apps.scope_other_read", { area: areaWords(area) });
  }
  if (action === "write") {
    const key = `apps.scope_write_${canonical}`;
    return hasKey(t, key) ? t(key) : t("apps.scope_other_write", { area: areaWords(area) });
  }
  return hasKey(t, own) ? t(own) : scope;
}

type Actor = AdminSchemas["EventActor"] | null | undefined;

/**
 * Who did something, for a line of history: "a staff member", "the app itself", …
 * @deprecated English only: pass the translator, `actorLabel(t, actor)`.
 */
export function actorLabel(actor: Actor): string;
/** Who did something, for a line of history, in `t`'s language. */
export function actorLabel(t: TFor<"apps">, actor: Actor): string;
export function actorLabel(first: TFor<"apps"> | Actor, second?: Actor): string {
  if (typeof first === "function") {
    const key = `apps.actor_${second?.type ?? "unknown"}`;
    return hasKey(first, key) ? first(key) : first("apps.actor_unknown");
  }
  switch (first?.type) {
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
