// The TypeScript manifest type and the published JSON Schema (docs/apps/manifest.schema.json,
// written from hypha-apps' `ManifestDocument`, which denies unknown fields) agree: a sample
// with every field the type has is typed against it (so a field the type gains must be
// added here), the schema accepts it (so the type has no field Hypha would refuse), and
// the sample has every property the schema lists (so the type lacks none).

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";

import { Ajv2020 } from "ajv/dist/2020.js";

import { MANIFEST_SCHEMA_URL, defineManifest, type AppManifest } from "./manifest.ts";

/** `T` with every optional field required, all the way down. */
type DeepRequired<T> = T extends string | number | boolean
  ? T
  : T extends readonly (infer U)[]
    ? readonly DeepRequired<U>[]
    : { [K in keyof T]-?: DeepRequired<Exclude<T[K], undefined>> };

const full = {
  $schema: MANIFEST_SCHEMA_URL,
  key: "app_stripe",
  name: "Stripe",
  version: "1.0.0",
  app_url: "https://stripe-app.example",
  redirect_uris: ["/oauth/callback"],
  scopes: ["payments:write", "orders:read", "customer_data:email"],
  scope_reasons: { "customer_data:email": "Stripe sends the receipt" },
  privacy_url: "/privacy",
  publisher: "Hypha",
  icon: "/icon.png",
  webhooks: { url: "/webhooks", events: ["order.cancelled", "app.*"] },
  extension_points: [{ point: "payments.start", version: 1, url: "/ext" }],
  settings: {
    schema: { type: "object", properties: { publishable_key: { type: "string" } } },
    secret_fields: ["secret_key"],
  },
  admin: {
    pages: [{ id: "dashboard", title: "Stripe", url: "/admin", nav: "apps" }],
    blocks: [{ id: "payment", target: "order.payment", url: "/blocks/payment" }],
    actions: [
      {
        id: "capture",
        target: "payment",
        label: "Capture",
        point: "admin.action",
        url: "/actions/capture",
        input_schema: { type: "object", properties: { amount: { type: "integer" } } },
      },
    ],
    proxy: "/admin-proxy",
  },
  storefront: {
    elements: [
      {
        id: "card",
        target: "checkout.payment",
        module: "/elements/card.js",
        tag: "stripe-payment",
      },
    ],
    proxy: "/storefront-proxy",
  },
} satisfies DeepRequired<AppManifest>;

const schema = JSON.parse(
  await readFile(new URL("../../../docs/apps/manifest.schema.json", import.meta.url), "utf8"),
) as Record<string, unknown>;

const ajv = new Ajv2020({ allErrors: true, strict: false });
const accepts = ajv.compile(schema);

test("the schema accepts a manifest with every field the type has", () => {
  const manifest: AppManifest = defineManifest(full);
  assert.equal(accepts(manifest), true, JSON.stringify(accepts.errors, null, 2));
});

test("the schema refuses a field the type doesn't have", () => {
  assert.equal(accepts({ ...full, icon_url: "/icon.png" }), false);
});

type Node = Record<string, unknown>;

const isNode = (value: unknown): value is Node =>
  typeof value === "object" && value !== null && !Array.isArray(value);

/** The node `$ref` points at, or the non-null branch of an `Option`'s `anyOf`. */
function resolve(node: Node): Node {
  const ref = node.$ref;
  if (typeof ref === "string") {
    const name = ref.replace("#/$defs/", "");
    const defs = schema.$defs;
    const target = isNode(defs) ? defs[name] : undefined;
    assert.ok(isNode(target), `unknown ${ref}`);
    return resolve(target);
  }
  const branches = node.anyOf ?? node.oneOf;
  if (Array.isArray(branches)) {
    const real = branches.filter(isNode).find((b) => b.type !== "null");
    if (real) return resolve(real);
  }
  return node;
}

/** Every schema property path missing from `value`. */
function missing(node: Node, value: unknown, path: string): string[] {
  const resolved = resolve(node);
  const items = resolved.items;
  if (Array.isArray(value)) {
    return isNode(items) && value.length > 0 ? missing(items, value[0], `${path}[0]`) : [];
  }
  const properties = resolved.properties;
  if (!isNode(properties) || !isNode(value)) return [];
  return Object.entries(properties).flatMap(([key, sub]) => {
    const at = path ? `${path}.${key}` : key;
    if (!(key in value)) return [at];
    // Free-form objects (a settings or input JSON Schema) aren't the manifest's shape.
    if (!isNode(sub) || resolve(sub).additionalProperties === true) return [];
    return missing(sub, value[key], at);
  });
}

test("the type has every property the schema lists", () => {
  assert.deepEqual(missing(schema, full, ""), []);
});
