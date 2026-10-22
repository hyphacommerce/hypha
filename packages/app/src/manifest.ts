// The app manifest, `hypha-app.json`. These types follow the published JSON Schema,
// `docs/apps/manifest.schema.json` (written from hypha-apps' `AppManifest`), which is
// what Hypha validates against; keep the two in step when either changes. URLs that
// start with `/` are relative to `app_url`.

/** A scope name, e.g. `orders:read` or `customer_data:email`. */
export type Scope = string;

/** The personal-data scopes: without them those fields are absent. */
export const PERSONAL_DATA_SCOPES = [
  "customer_data:name",
  "customer_data:email",
  "customer_data:phone",
  "customer_data:address",
  "customer_data:profile",
] as const;
export type PersonalDataScope = (typeof PERSONAL_DATA_SCOPES)[number];

/** The extension points this SDK knows. Others may exist on newer deployments. */
export type ExtensionPointName =
  | "shipping.rates"
  | "payments.start"
  | "payments.status"
  | "payments.capture"
  | "payments.void"
  | "payments.refund"
  | "search.query"
  | "admin.action";

/** The admin targets. */
export type AdminTarget =
  | "apps.page"
  | "settings.app"
  | "order.details"
  | "order.payment"
  | "order.fulfillment"
  | "product.details"
  | "customer.details"
  | "payment";

/** The storefront targets. */
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
  /** A JSON Schema for a frontend to render the app's settings form from. */
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
  /** Answers a declarative block: Hypha fetches it, signed, and checks it. */
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
  /** The app proxy's base for admins, behind `/admin/apps/{app}/proxy/…`. */
  proxy?: string;
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
  /** The app proxy's base path. */
  proxy?: string;
}

/** What a custom field is on. */
export type CustomFieldOwnerType = "product" | "variant" | "category" | "customer" | "order";

/** The types a field may also be a list of (`list.<type>`). */
export type ListableCustomFieldType =
  | "text"
  | "integer"
  | "decimal"
  | "date"
  | "date_time"
  | "url"
  | "money"
  | "measurement"
  | "choice"
  | "reference";

/** A custom field's type. It can't change once installed. */
export type CustomFieldType =
  | ListableCustomFieldType
  | "multiline_text"
  | "rich_text"
  | "boolean"
  | "json"
  | `list.${ListableCustomFieldType}`;

/**
 * A custom field the app keeps on Hypha's objects, always in its own namespace
 * (`app_<name>.<key>`): created when staff approve the app, updated when they approve a
 * refreshed manifest, deleted with its values when a refreshed manifest drops it.
 */
export interface ManifestCustomField {
  owner_type: CustomFieldOwnerType;
  /** `[a-z][a-z0-9_]{1,63}`, unique per owner type. */
  key: string;
  type: CustomFieldType;
  /** Its label on forms, at most 255 characters. */
  name: string;
  /** Help text, at most 1,000 characters. */
  description?: string;
  /** The type's validations (`max_length`, `choices`, …). */
  validations?: Record<string, unknown>;
  access?: {
    /** `read` (default): staff only read the values; `read_write`: they edit them too. */
    admin?: "read" | "read_write";
    /** `none` (default) or `read`: storefronts see the values. */
    storefront?: "none" | "read";
  };
  /** Only the app and staff read the values. */
  private?: boolean;
  /** About a person (customer and order fields only); needs `customer_data:profile`. */
  personal?: boolean;
  capabilities?: {
    /** Lists can filter on it. */
    filterable?: boolean;
    /** No two owners hold the same value (`text`, `integer`, `url`). */
    unique?: boolean;
  };
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
  /** The app's privacy policy, linked from the consent screen. */
  privacy_url: string;
  /** Who publishes the app, shown on the consent screen. */
  publisher?: string;
  /** A square image's URL for the consent screen and app lists. */
  icon?: string;
  webhooks?: ManifestWebhooks;
  extension_points?: ManifestExtensionPoint[];
  settings?: ManifestSettings;
  admin?: ManifestAdmin;
  storefront?: ManifestStorefront;
  /** Custom fields in the app's namespace. */
  custom_fields?: ManifestCustomField[];
}

/** The manifest's schema URL. */
export const MANIFEST_SCHEMA_URL = "https://hypha.dev/schemas/app-manifest/1.json";

/** Types a manifest in place (no runtime checks: Hypha validates it on install). */
export function defineManifest<const M extends AppManifest>(manifest: M): M {
  return manifest;
}
