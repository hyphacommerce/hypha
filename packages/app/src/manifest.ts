// The app manifest, `hypha-app.json` (design note 0029, §4, AP4). These types follow the
// published JSON Schema, `docs/apps/manifest.schema.json` (written from hypha-apps'
// `AppManifest`), which is what Hypha validates against; keep the two in step when
// either changes. URLs that start with `/` are relative to `app_url`.

/** A scope name, e.g. `orders:read` or `customer_data:email` (AP10). */
export type Scope = string;

/** The personal-data scopes (design note 0029, §4.2): without them those fields are absent. */
export const PERSONAL_DATA_SCOPES = [
  "customer_data:name",
  "customer_data:email",
  "customer_data:phone",
  "customer_data:address",
] as const;
export type PersonalDataScope = (typeof PERSONAL_DATA_SCOPES)[number];

/** The extension points this SDK knows (§3.1). Others may exist on newer deployments. */
export type ExtensionPointName =
  | "shipping.rates"
  | "payments.start"
  | "payments.capture"
  | "payments.void"
  | "payments.refund"
  | "search.query";

/** The admin targets (§5). */
export type AdminTarget =
  | "apps.page"
  | "settings.app"
  | "order.details"
  | "order.payment"
  | "order.fulfillment"
  | "product.details"
  | "customer.details"
  | "payment";

/** The storefront targets (§5). */
export type StorefrontTarget =
  | "checkout.payment"
  | "product.block"
  | "cart.block"
  | "order.confirmation";

/** Any target; a string so a newer catalog's targets still type-check. */
export type Target = AdminTarget | StorefrontTarget | (string & {});

export interface ManifestWebhooks {
  /** Where deliveries go. */
  url: string;
  /** Event types, e.g. `order.cancelled`. */
  events: string[];
}

export interface ManifestExtensionPoint {
  point: ExtensionPointName | (string & {});
  version: number;
  url: string;
}

export interface ManifestSettings {
  /** A JSON Schema for a frontend to render the app's settings form from (§6.1). */
  schema: Record<string, unknown>;
  /** Fields a form must treat as secrets (masked, never echoed back). */
  secret_fields?: string[];
}

export interface ManifestAdminPage {
  id: string;
  title: string;
  url: string;
  /** Where an admin lists the page, e.g. `apps`. */
  nav?: string;
}

export interface ManifestAdminBlock {
  id: string;
  target: Target;
  /** Answers a declarative block (§6.2), fetched through the proxy. */
  url: string;
}

export interface ManifestAdminAction {
  id: string;
  target: Target;
  label: string;
  /** The extension point pressing it calls, or `url`. */
  point?: ExtensionPointName | (string & {});
  url?: string;
  /** A JSON Schema for the action's input form, when it takes any. */
  input_schema?: Record<string, unknown>;
}

export interface ManifestAdmin {
  pages?: ManifestAdminPage[];
  blocks?: ManifestAdminBlock[];
  actions?: ManifestAdminAction[];
}

export interface ManifestStorefrontElement {
  id: string;
  target: Target;
  /** The ES module defining the custom element. */
  module: string;
  /** The custom element's tag, e.g. `stripe-payment`. */
  tag: string;
}

export interface ManifestStorefront {
  elements?: ManifestStorefrontElement[];
  /** The app proxy's base path (§8). */
  proxy?: string;
}

export interface AppManifest {
  $schema?: string;
  /** `app_<name>`: lowercase letters, digits and `_`. */
  key: string;
  name: string;
  /** The app's own version, semver. */
  version: string;
  app_url: string;
  /** OAuth redirect URIs, matched exactly. */
  redirect_uris: string[];
  scopes: Scope[];
  /** Why the app needs a scope, shown on the consent screen (required in practice for personal data). */
  scope_reasons?: Partial<Record<Scope, string>>;
  /** The app's privacy policy, linked from the consent screen (§4.2). */
  privacy_url: string;
  /** Who publishes the app, shown on the consent screen. */
  publisher?: string;
  /** An icon URL for the consent screen and app lists. */
  icon_url?: string;
  webhooks?: ManifestWebhooks;
  extension_points?: ManifestExtensionPoint[];
  settings?: ManifestSettings;
  admin?: ManifestAdmin;
  storefront?: ManifestStorefront;
}

/** The manifest's schema URL. */
export const MANIFEST_SCHEMA_URL = "https://hypha.dev/schemas/app-manifest/1.json";

/** Types a manifest in place (no runtime checks: Hypha validates it on install). */
export function defineManifest<const M extends AppManifest>(manifest: M): M {
  return manifest;
}
