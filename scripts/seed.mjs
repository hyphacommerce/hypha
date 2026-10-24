#!/usr/bin/env node
// A demo store for the admin and the storefront: "Mycelium Works", a mycology company
// (somewhere in Europe, deliberately unnamed) selling grow kits, live cultures, lab equipment, materials grown from
// mycelium and courses. It's built to show what a catalogue can carry beyond shirts and
// mugs: sparse variants (only the species and sizes that exist), per-variant shipping (an
// eBook beside a paperback), digital products, perishables, and custom fields on
// products, variants, categories, customers and orders, some shown to shoppers and used
// as search filters, some for staff only, and references between products.
//
// It goes through the admin and storefront APIs like a merchant and their shoppers
// would, so every rule applies: store details, four markets (EUR, GBP, USD, RSD) with
// tax regions, price lists and fixed prices, two warehouses, shipping zones with rates,
// a category tree, products with stock (some sold out), publishing, search filters, a
// publishable key, the storefront's webhook, payment methods (cash on delivery in
// Serbia, bank transfer everywhere), and a few orders in different states, some paid.
//
// Run it against a fresh store (it stops when products already exist):
//
//   HYPHA_DEV_DB=hypha_demo HYPHA_OWNER_EMAIL=owner@example.com \
//     HYPHA_OWNER_PASSWORD=demo-owner-password pnpm dev
//   pnpm seed                     # in another terminal
//
// HYPHA_URL (default http://127.0.0.1:3917), HYPHA_SEED_EMAIL and HYPHA_SEED_PASSWORD
// (default the owner above) say where and as whom. Images need the API's file storage
// (the compose S3 server under `cargo run`); without it products get none. Each image is
// `scripts/seed-assets/images/<name>.{webp,jpg,jpeg,png}` when that file exists (the prompts
// that make them are in `scripts/seed-assets/PROMPTS.md`), else a plain shot in the
// product's colour.
//
// `pnpm seed --images` on a store this script seeded uploads the images again (new photos
// replacing placeholders) and points products, variants and categories at them.

import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { deflateSync } from "node:zlib";

const BASE = (process.env.HYPHA_URL ?? "http://127.0.0.1:3917").replace(/\/$/, "");
const EMAIL = process.env.HYPHA_SEED_EMAIL ?? process.env.HYPHA_OWNER_EMAIL ?? "owner@example.com";
const PASSWORD =
  process.env.HYPHA_SEED_PASSWORD ?? process.env.HYPHA_OWNER_PASSWORD ?? "demo-owner-password";
const STOREFRONT_URL = process.env.STOREFRONT_URL ?? "http://localhost:3002";

// ---------- the API ----------

let token;

class ApiError extends Error {
  constructor(method, path, status, body) {
    super(
      `${method} ${path} → ${status} ${body?.code ?? ""}: ${body?.error ?? JSON.stringify(body)}`,
    );
    this.status = status;
    this.code = body?.code;
  }
}

async function call(method, path, body, { key, headers = {} } = {}) {
  const response = await fetch(`${BASE}${path}`, {
    method,
    headers: {
      ...(body !== undefined ? { "content-type": "application/json" } : {}),
      ...(key ? { "x-publishable-key": key } : token ? { authorization: `Bearer ${token}` } : {}),
      ...headers,
    },
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
  });
  const text = await response.text();
  const json = text ? JSON.parse(text) : undefined;
  if (!response.ok) throw new ApiError(method, path, response.status, json);
  return json;
}

const admin = {
  get: (path) => call("GET", path),
  post: (path, body) => call("POST", path, body ?? {}),
  put: (path, body) => call("PUT", path, body),
  patch: (path, body) => call("PATCH", path, body),
  delete: (path) => call("DELETE", path),
};

// ---------- deterministic randomness, money ----------

let state = 20261017;
/** The same "random" store every run. */
function random() {
  state = (state * 1664525 + 1013904223) % 4294967296;
  return state / 4294967296;
}
const pick = (items) => items[Math.floor(random() * items.length)];

const eur = (euros) => ({ amount: Math.round(euros * 100), currency: "EUR" });
/** A share of a price, rounded to the minor unit (0.8 is 20% off). */
const share = (money, factor) => ({
  amount: Math.round(money.amount * factor),
  currency: money.currency,
});

/** A base price (EUR cents) in another market's currency, rounded like a shop would. */
function convert(cents, currency) {
  const euros = cents / 100;
  switch (currency) {
    case "GBP":
      return { amount: Math.max(1, Math.ceil(euros * 0.86)) * 100 - 1, currency }; // £x.99
    case "USD":
      return { amount: Math.ceil(euros * 1.08) * 100, currency }; // $x.00
    case "RSD":
      return { amount: Math.ceil((euros * 117.2) / 100) * 100 * 100, currency }; // 3,500.00
    default:
      return { amount: cents, currency };
  }
}

// ---------- images ----------

const crcTable = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});
function crc32(bytes) {
  let c = 0xffffffff;
  for (const b of bytes) c = crcTable[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}
function chunk(type, data) {
  const length = Buffer.alloc(4);
  length.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type, "ascii"), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([length, body, crc]);
}

/** An 800 × 1000 product shot: the colour with soft light from the top and a floor. */
function productImage(hex) {
  const [r, g, b] = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16));
  const width = 800;
  const height = 1000;
  const raw = Buffer.alloc((width * 3 + 1) * height);
  for (let y = 0; y < height; y++) {
    const row = y * (width * 3 + 1);
    raw[row] = 0;
    const light = y < 820 ? 1.12 - (y / 820) * 0.2 : 0.78;
    for (let x = 0; x < width; x++) {
      const vignette = 1 - 0.12 * ((x - width / 2) / (width / 2)) ** 2;
      const f = light * vignette;
      const i = row + 1 + x * 3;
      raw[i] = Math.min(255, Math.round(r * f));
      raw[i + 1] = Math.min(255, Math.round(g * f));
      raw[i + 2] = Math.min(255, Math.round(b * f));
    }
  }
  const header = Buffer.alloc(13);
  header.writeUInt32BE(width, 0);
  header.writeUInt32BE(height, 4);
  header[8] = 8; // bit depth
  header[9] = 2; // RGB
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", header),
    chunk("IDAT", deflateSync(raw, { level: 9 })),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

let mediaWorks = true;

const IMAGES = join(import.meta.dirname, "seed-assets", "images");
const MIME = { webp: "image/webp", jpg: "image/jpeg", jpeg: "image/jpeg", png: "image/png" };

/** The image called `name`: its file when there is one, else a plain shot in `hex`. */
async function imageFile(name, hex) {
  for (const [extension, mime] of Object.entries(MIME)) {
    const bytes = await readFile(join(IMAGES, `${name}.${extension}`)).catch(() => null);
    if (bytes) return { bytes, mime, filename: `${name}.${extension}` };
  }
  return { bytes: productImage(hex), mime: "image/png", filename: `${name}.png` };
}

/** Uploads the image called `name` (`hex` its colour when there's no file); its asset's
 * ID, or `null` without file storage. */
async function upload(name, hex, alt) {
  if (!mediaWorks) return null;
  const file = await imageFile(name, hex);
  let created;
  try {
    created = await admin.post("/admin/assets", {
      filename: file.filename,
      mime_type: file.mime,
      byte_size: file.bytes.length,
    });
  } catch (e) {
    if (e.code === "media_not_configured") {
      console.warn("  (no file storage: products get no images)");
      mediaWorks = false;
      return null;
    }
    throw e;
  }
  const headers = Object.fromEntries(created.upload.headers.map((h) => [h.name, h.value]));
  const stored = await fetch(created.upload.url, { method: "PUT", headers, body: file.bytes });
  if (!stored.ok) throw new Error(`uploading ${file.filename} to storage failed: ${stored.status}`);
  const asset = await admin.post(`/admin/assets/${created.asset.id}/complete`);
  await admin.patch(`/admin/assets/${asset.id}`, { alt });
  return asset.id;
}

// ---------- the catalogue ----------

/** The brand's colours, for the plain shots that stand in for a missing image. */
const SWATCH = {
  lime: "#B8E948",
  moss: "#5F7E01",
  forest: "#304204",
  pale: "#EAFBCF",
  stone: "#E6E8E5",
  bark: "#6A6C68",
  charcoal: "#30322F",
  oyster: "#D4D5D2",
  blue: "#A3B8C9",
  pink: "#E8B4B8",
  gold: "#E8C66A",
  cream: "#F3EAD7",
  clay: "#B8775A",
  rust: "#953F21",
};

const choices = (pairs) => pairs.map(([key, label]) => ({ key, label }));

const SPECIES = choices([
  ["pearl_oyster", "Pearl oyster"],
  ["blue_oyster", "Blue oyster"],
  ["pink_oyster", "Pink oyster"],
  ["golden_oyster", "Golden oyster"],
  ["king_oyster", "King oyster"],
  ["lions_mane", "Lion's mane"],
  ["shiitake", "Shiitake"],
  ["reishi", "Reishi"],
  ["chestnut", "Chestnut"],
]);
const speciesKey = (label) => SPECIES.find((s) => s.label === label).key;

/** Shown to shoppers. */
const SHOWN = { admin: "read_write", storefront: "read" };
/** Staff only. */
const STAFF = { admin: "read_write", storefront: "none" };

/**
 * The merchant's own fields (namespace `custom`), in the order forms show them. What a
 * shopper filters by is `filterable` and shown, and listed in the search facets below.
 */
const FIELDS = [
  // Products: what a grower, a lab or a designer compares.
  {
    owner_type: "product",
    key: "difficulty",
    type: "choice",
    name: "Difficulty",
    description: "How much experience it asks for.",
    validations: {
      choices: choices([
        ["beginner", "Beginner"],
        ["intermediate", "Intermediate"],
        ["advanced", "Advanced"],
      ]),
    },
    access: SHOWN,
    capabilities: { filterable: true },
  },
  {
    owner_type: "product",
    key: "live_culture",
    type: "boolean",
    name: "Live culture",
    description:
      "Alive when it ships: it leaves on Mondays to Wednesdays, never to sit in a depot over a weekend.",
    access: SHOWN,
    capabilities: { filterable: true },
  },
  {
    owner_type: "product",
    key: "first_harvest_days",
    type: "integer",
    name: "Days to first harvest",
    validations: { min: 1, max: 365 },
    access: SHOWN,
  },
  {
    owner_type: "product",
    key: "expected_yield",
    type: "measurement",
    name: "Expected yield per flush",
    validations: { dimension: "weight" },
    access: SHOWN,
  },
  {
    owner_type: "product",
    key: "substrate",
    type: "list.choice",
    name: "Substrate",
    validations: {
      choices: choices([
        ["hardwood", "Hardwood sawdust"],
        ["soy_hulls", "Soy hulls"],
        ["straw", "Straw"],
        ["coffee", "Spent coffee grounds"],
        ["rye", "Rye grain"],
        ["agar", "Agar"],
        ["hemp", "Hemp hurd"],
      ]),
      unique_items: true,
    },
    access: SHOWN,
  },
  {
    owner_type: "product",
    key: "certifications",
    type: "list.choice",
    name: "Certifications",
    validations: {
      choices: choices([
        ["eu_organic", "EU organic"],
        ["home_compostable", "Home compostable (TÜV OK compost HOME)"],
        ["oeko_tex", "OEKO-TEX Standard 100"],
        ["vegan", "Vegan (no animal inputs)"],
        ["en_1822", "Filters tested to EN 1822"],
        ["ce", "CE marked"],
      ]),
      unique_items: true,
    },
    access: SHOWN,
    capabilities: { filterable: true },
  },
  {
    owner_type: "product",
    key: "fire_rating",
    type: "choice",
    name: "Fire rating (EN 13501-1)",
    validations: {
      choices: choices([
        ["b_s1_d0", "B-s1, d0"],
        ["c_s2_d0", "C-s2, d0"],
        ["e", "E"],
      ]),
    },
    access: SHOWN,
  },
  {
    owner_type: "product",
    key: "sound_absorption",
    type: "decimal",
    name: "Sound absorption (NRC)",
    description: "The noise reduction coefficient: 0 reflects everything, 1 absorbs everything.",
    validations: { min: "0", max: "1", max_scale: 2 },
    access: SHOWN,
  },
  {
    owner_type: "product",
    key: "warranty_months",
    type: "integer",
    name: "Warranty (months)",
    validations: { min: 0, max: 120 },
    access: SHOWN,
  },
  {
    owner_type: "product",
    key: "spec_sheet",
    type: "url",
    name: "Spec sheet",
    validations: { schemes: ["https"] },
    access: SHOWN,
  },
  {
    owner_type: "product",
    key: "works_with",
    type: "list.reference",
    name: "Works with",
    description: "Products it's made to be used with.",
    validations: { kind: "product", max_items: 6 },
    access: SHOWN,
  },
  {
    owner_type: "product",
    key: "care_guide",
    type: "rich_text",
    name: "Care guide",
    access: SHOWN,
  },
  {
    owner_type: "product",
    key: "grown_in",
    type: "text",
    name: "Grown in",
    validations: { max_length: 120 },
    access: SHOWN,
  },
  {
    owner_type: "product",
    key: "next_session",
    type: "date_time",
    name: "Next live session",
    access: SHOWN,
  },
  {
    owner_type: "product",
    key: "duration_hours",
    type: "decimal",
    name: "Duration (hours)",
    validations: { min: "0", max_scale: 1 },
    access: SHOWN,
  },
  {
    owner_type: "product",
    key: "supplier_code",
    type: "text",
    name: "Supplier code",
    description: "The supplier's own SKU, for reordering.",
    validations: { max_length: 64 },
    access: STAFF,
    capabilities: { filterable: true, unique: true },
  },
  {
    owner_type: "product",
    key: "landed_cost",
    type: "money",
    name: "Landed cost",
    description: "What one unit costs us delivered to the warehouse, duties included.",
    validations: { currencies: ["EUR"] },
    access: STAFF,
  },

  // Variants: what differs between one size, strain or plug and the next.
  {
    owner_type: "variant",
    key: "species",
    type: "choice",
    name: "Species",
    validations: { choices: SPECIES },
    access: SHOWN,
    capabilities: { filterable: true },
  },
  {
    owner_type: "variant",
    key: "volume",
    type: "measurement",
    name: "Volume",
    validations: { dimension: "volume" },
    access: SHOWN,
  },
  {
    owner_type: "variant",
    key: "generation",
    type: "integer",
    name: "Culture generation",
    description: "Transfers since the master culture: the lower, the more vigorous.",
    validations: { min: 1, max: 10 },
    access: SHOWN,
  },
  {
    owner_type: "variant",
    key: "lab_tested_on",
    type: "date",
    name: "Contamination tested",
    access: SHOWN,
  },
  {
    owner_type: "variant",
    key: "capacity",
    type: "measurement",
    name: "Capacity",
    validations: { dimension: "volume" },
    access: SHOWN,
  },
  {
    owner_type: "variant",
    key: "voltage",
    type: "choice",
    name: "Voltage",
    validations: {
      choices: choices([
        ["230v", "220–240 V, 50 Hz"],
        ["120v", "110–120 V, 60 Hz"],
      ]),
    },
    access: SHOWN,
  },
  {
    owner_type: "variant",
    key: "thickness",
    type: "measurement",
    name: "Thickness",
    validations: { dimension: "length" },
    access: SHOWN,
  },
  {
    owner_type: "variant",
    key: "width",
    type: "measurement",
    name: "Width",
    validations: { dimension: "length" },
    access: SHOWN,
  },
  {
    owner_type: "variant",
    key: "height",
    type: "measurement",
    name: "Height",
    validations: { dimension: "length" },
    access: SHOWN,
  },
  {
    owner_type: "variant",
    key: "lot",
    type: "text",
    name: "Lot",
    description: "The production lot on shelf now, for recalls and lab records.",
    validations: { pattern: "^MW-[0-9]{4}-[0-9]{3}$" },
    access: STAFF,
    capabilities: { filterable: true },
  },

  // Categories: a buying guide at the top of the category page.
  {
    owner_type: "category",
    key: "buying_guide",
    type: "rich_text",
    name: "Buying guide",
    access: SHOWN,
  },

  // Customers: who they are to the business (with the suggested labels and notes below).
  {
    owner_type: "customer",
    key: "business_type",
    type: "choice",
    name: "Business type",
    validations: {
      choices: choices([
        ["home", "Home grower"],
        ["farm", "Mushroom farm"],
        ["lab", "Research lab"],
        ["studio", "Design studio"],
        ["restaurant", "Restaurant"],
      ]),
    },
    access: STAFF,
    capabilities: { filterable: true },
  },
  {
    owner_type: "customer",
    key: "vat_id",
    type: "text",
    name: "VAT ID",
    validations: { pattern: "^[A-Z]{2}[0-9A-Z]{2,13}$" },
    access: STAFF,
    capabilities: { filterable: true, unique: true },
  },

  // Orders: what the warehouse needs to pack it right.
  {
    owner_type: "order",
    key: "cold_pack",
    type: "boolean",
    name: "Ships with a cold pack",
    access: STAFF,
    capabilities: { filterable: true },
  },
  {
    owner_type: "order",
    key: "gift_message",
    type: "multiline_text",
    name: "Gift message",
    validations: { max_length: 500 },
    access: STAFF,
    personal: true,
  },
];

/** Search filters on the storefront. */
const FACETS = [
  { key: "field.custom.species", label: "Species" },
  { key: "field.custom.difficulty", label: "Difficulty" },
  { key: "field.custom.live_culture", label: "Live culture" },
  { key: "field.custom.certifications", label: "Certifications" },
  { key: "product_type", label: "Type" },
  { key: "option.colour", label: "Colour" },
  { key: "vendor", label: "Maker" },
  // Counted for the storefront's "On sale" toggle.
  { key: "on_sale", label: "On sale" },
];

const CATEGORIES = [
  {
    name: "Grow",
    handle: "grow",
    image: "category-grow",
    swatch: SWATCH.moss,
    guide:
      "<p>New to growing? Start with an <strong>oyster kit</strong>: it fruits in about ten days on a kitchen counter. Lion's mane and shiitake take more patience; reishi is for when you want a project.</p>",
    children: [
      { name: "Grow kits", handle: "grow-kits" },
      { name: "Cultures & spawn", handle: "cultures" },
      { name: "Substrates", handle: "substrates" },
    ],
  },
  {
    name: "Lab",
    handle: "lab",
    image: "category-lab",
    swatch: SWATCH.charcoal,
    guide:
      "<p>Clean work is most of mycology. A <strong>still air box</strong> gets a beginner far; a <strong>flow hood</strong> and a <strong>pressure sterilizer</strong> turn a hobby into a lab.</p>",
    children: [
      { name: "Sterilization", handle: "sterilization" },
      { name: "Clean air", handle: "clean-air" },
      { name: "Tools", handle: "tools" },
    ],
  },
  {
    name: "Materials",
    handle: "materials",
    image: "category-materials",
    swatch: SWATCH.clay,
    guide:
      "<p>Mycelium grows into whatever shape it's given, then is dried to stop it. The result replaces leather, foam and plastic, and goes back to the soil at the end.</p>",
    children: [
      { name: "Mycelium leather", handle: "leather" },
      { name: "Packaging", handle: "packaging" },
      { name: "Acoustic panels", handle: "acoustics" },
      { name: "Objects", handle: "objects" },
    ],
  },
  {
    name: "Learn",
    handle: "learn",
    image: "category-learn",
    swatch: SWATCH.lime,
    guide: "<p>From your first kit to your own cultures: courses and books by our lab team.</p>",
    children: [
      { name: "Courses", handle: "courses" },
      { name: "Books", handle: "books" },
    ],
  },
];

/** A product's variants: each `of` names its option values; only those listed exist. */
const v = (of, price, more = {}) => ({ of, price, ...more });
const ml = (value) => ({ value: String(value), unit: "ml" });
const l = (value) => ({ value: String(value), unit: "l" });
const g = (value) => ({ value: String(value), unit: "g" });
const mm = (value) => ({ value: String(value), unit: "mm" });
const cm = (value) => ({ value: String(value), unit: "cm" });

const PRODUCTS = [
  // ----- Grow kits -----
  {
    title: "Oyster Mushroom Grow Kit",
    handle: "oyster-grow-kit",
    type: "Grow kit",
    vendor: "Mycelium Works",
    tags: ["bestseller", "gift"],
    categories: ["grow-kits"],
    description:
      "<p>A block of hardwood and soy hulls, already colonised. Cut the window, mist twice a day, and harvest your first cluster in about ten days, then again two weeks later.</p>",
    options: {
      Species: ["Pearl oyster", "Blue oyster", "Pink oyster", "Golden oyster"],
      Size: ["Starter", "Family"],
    },
    // Pink and golden oysters fruit fast and keep badly: the starter block only.
    variants: [
      v({ Species: "Pearl oyster", Size: "Starter" }, 24, { weight: 1700 }),
      v({ Species: "Pearl oyster", Size: "Family" }, 39, { weight: 3400 }),
      v({ Species: "Blue oyster", Size: "Starter" }, 24, { weight: 1700 }),
      v({ Species: "Blue oyster", Size: "Family" }, 39, { weight: 3400 }),
      v({ Species: "Pink oyster", Size: "Starter" }, 26, { weight: 1700 }),
      v({ Species: "Golden oyster", Size: "Starter" }, 26, { weight: 1700 }),
    ],
    variantFields: (o) => ({ "custom.species": speciesKey(o.Species) }),
    imageBy: "Species",
    image: "oyster-grow-kit",
    swatchBy: {
      "Pearl oyster": SWATCH.oyster,
      "Blue oyster": SWATCH.blue,
      "Pink oyster": SWATCH.pink,
      "Golden oyster": SWATCH.gold,
    },
    fields: {
      "custom.difficulty": "beginner",
      "custom.live_culture": true,
      "custom.first_harvest_days": 10,
      "custom.expected_yield": g(450),
      "custom.substrate": ["hardwood", "soy_hulls"],
      "custom.grown_in": "Our lab and farm",
      "custom.care_guide":
        "<ol><li>Cut an X in the bag's window.</li><li>Mist the cut twice a day; keep it out of direct sun.</li><li>Harvest when the caps flatten, twisting the whole cluster off.</li><li>Rest the block two weeks, soak it overnight, and it fruits again.</li></ol>",
      "custom.supplier_code": "MW-OYS-BLK",
      "custom.landed_cost": eur(7.4),
    },
  },
  {
    title: "Lion's Mane Grow Kit",
    handle: "lions-mane-grow-kit",
    type: "Grow kit",
    vendor: "Mycelium Works",
    tags: ["gift"],
    categories: ["grow-kits"],
    description:
      "<p>Lion's mane grows as a white cascade of soft spines and tastes like crab when it's fried in butter. Slower than an oyster, and worth it.</p>",
    options: { Size: ["Starter", "Family"] },
    variants: [
      v({ Size: "Starter" }, 28, { weight: 1700 }),
      v({ Size: "Family" }, 45, { weight: 3400 }),
    ],
    variantFields: () => ({ "custom.species": "lions_mane" }),
    image: "lions-mane-grow-kit",
    swatch: SWATCH.cream,
    fields: {
      "custom.difficulty": "intermediate",
      "custom.live_culture": true,
      "custom.first_harvest_days": 18,
      "custom.expected_yield": g(350),
      "custom.substrate": ["hardwood", "soy_hulls"],
      "custom.grown_in": "Our lab and farm",
      "custom.supplier_code": "MW-LIO-BLK",
      "custom.landed_cost": eur(8.9),
    },
  },
  {
    title: "Shiitake Log",
    handle: "shiitake-log",
    type: "Grow kit",
    vendor: "Mycelium Works",
    tags: ["outdoor"],
    categories: ["grow-kits"],
    description:
      "<p>An oak log drilled and plugged with shiitake spawn. Keep it shaded in the garden; it fruits each spring and autumn for four to six years.</p>",
    options: { Length: ["40 cm", "60 cm"] },
    variants: [
      v({ Length: "40 cm" }, 34, { weight: 6000 }),
      v({ Length: "60 cm" }, 49, { weight: 9000 }),
    ],
    variantFields: () => ({ "custom.species": "shiitake" }),
    image: "shiitake-log",
    swatch: SWATCH.bark,
    fields: {
      "custom.difficulty": "intermediate",
      "custom.live_culture": true,
      "custom.first_harvest_days": 240,
      "custom.expected_yield": g(300),
      "custom.substrate": ["hardwood"],
      "custom.grown_in": "Our forest plots",
    },
  },
  {
    title: "Reishi Antler Kit",
    handle: "reishi-antler-kit",
    type: "Grow kit",
    vendor: "Mycelium Works",
    tags: ["project"],
    categories: ["grow-kits"],
    description:
      "<p>Starved of fresh air, reishi grows lacquered red antlers instead of caps. Grow them for tea, or dry them as sculpture.</p>",
    price: 32,
    weight: 1600,
    variantFields: () => ({ "custom.species": "reishi" }),
    image: "reishi-antler-kit",
    swatch: SWATCH.rust,
    fields: {
      "custom.difficulty": "advanced",
      "custom.live_culture": true,
      "custom.first_harvest_days": 60,
      "custom.substrate": ["hardwood"],
    },
  },
  {
    title: "Fruiting Chamber",
    handle: "fruiting-chamber",
    type: "Equipment",
    vendor: "Mycelium Works",
    tags: ["equipment"],
    categories: ["grow-kits", "clean-air"],
    description:
      "<p>A shelved tent that holds humidity around your blocks. The ultrasonic version runs a fogger on a humidity sensor, so you can leave for a weekend.</p>",
    options: { Shelves: ["2 shelves", "4 shelves"], Humidifier: ["None", "Ultrasonic"] },
    // The fogger only comes with the tall chamber.
    variants: [
      v({ Shelves: "2 shelves", Humidifier: "None" }, 59, { weight: 4200 }),
      v({ Shelves: "4 shelves", Humidifier: "None" }, 89, { weight: 6800 }),
      v({ Shelves: "4 shelves", Humidifier: "Ultrasonic" }, 179, { weight: 8100 }),
    ],
    variantFields: (o) => ({ "custom.height": cm(o.Shelves === "2 shelves" ? 90 : 160) }),
    image: "fruiting-chamber",
    swatch: SWATCH.stone,
    fields: {
      "custom.difficulty": "beginner",
      "custom.warranty_months": 24,
      "custom.works_with": ["oyster-grow-kit", "lions-mane-grow-kit", "reishi-antler-kit"],
      "custom.spec_sheet": "https://myceliumworks.example/specs/fruiting-chamber.pdf",
      "custom.certifications": ["ce"],
      "custom.supplier_code": "GT-FC-2024",
      "custom.landed_cost": eur(31),
    },
  },

  // ----- Cultures & spawn -----
  {
    title: "Liquid Culture Syringe",
    handle: "liquid-culture-syringe",
    type: "Culture",
    vendor: "Mycelium Works Lab",
    tags: ["lab"],
    categories: ["cultures"],
    description:
      "<p>Living mycelium in a nutrient broth, drawn from our master cultures and tested clean on agar before it ships. Inject it into grain or a sterilised bag.</p>",
    options: {
      Species: [
        "Pearl oyster",
        "Blue oyster",
        "King oyster",
        "Lion's mane",
        "Shiitake",
        "Reishi",
        "Chestnut",
      ],
      Volume: ["10 ml", "20 ml"],
    },
    // Slow growers come in the small syringe only: a large one would age before it's used.
    variants: [
      ...["Pearl oyster", "Blue oyster", "King oyster", "Lion's mane", "Shiitake"].flatMap(
        (species) => [
          v({ Species: species, Volume: "10 ml" }, 14, { weight: 40 }),
          v({ Species: species, Volume: "20 ml" }, 22, { weight: 60 }),
        ],
      ),
      v({ Species: "Reishi", Volume: "10 ml" }, 16, { weight: 40 }),
      v({ Species: "Chestnut", Volume: "10 ml" }, 16, { weight: 40 }),
    ],
    variantFields: (o, i) => ({
      "custom.species": speciesKey(o.Species),
      "custom.volume": ml(o.Volume === "10 ml" ? 10 : 20),
      "custom.generation": 1 + (i % 2),
      "custom.lab_tested_on": "2026-10-12",
      "custom.lot": `MW-2026-${String(410 + i).padStart(3, "0")}`,
    }),
    image: "liquid-culture-syringe",
    swatch: SWATCH.pale,
    fields: {
      "custom.difficulty": "intermediate",
      "custom.live_culture": true,
      "custom.works_with": ["grain-spawn-bag", "still-air-box", "inoculation-kit"],
    },
  },
  {
    title: "Grain Spawn Bag",
    handle: "grain-spawn-bag",
    type: "Spawn",
    vendor: "Mycelium Works Lab",
    tags: ["lab", "bulk"],
    categories: ["cultures"],
    description:
      "<p>Sterilised rye fully colonised by the species you choose, with a filter patch so it breathes. Mix it into a bulk substrate to start a grow of your own.</p>",
    options: {
      Species: ["Pearl oyster", "Blue oyster", "King oyster", "Lion's mane", "Shiitake"],
      Weight: ["1 kg", "3 kg"],
    },
    // King oyster spawn goes to farms: the 3 kg bag only.
    variants: [
      ...["Pearl oyster", "Blue oyster", "Lion's mane", "Shiitake"].flatMap((species) => [
        v({ Species: species, Weight: "1 kg" }, 12, { weight: 1100 }),
        v({ Species: species, Weight: "3 kg" }, 29, { weight: 3200 }),
      ]),
      v({ Species: "King oyster", Weight: "3 kg" }, 32, { weight: 3200 }),
    ],
    variantFields: (o, i) => ({
      "custom.species": speciesKey(o.Species),
      "custom.lot": `MW-2026-${String(520 + i).padStart(3, "0")}`,
    }),
    image: "grain-spawn-bag",
    swatch: SWATCH.cream,
    fields: {
      "custom.difficulty": "intermediate",
      "custom.live_culture": true,
      "custom.substrate": ["rye"],
      "custom.works_with": ["hardwood-substrate", "coffee-substrate"],
    },
  },
  {
    title: "Agar Plates",
    handle: "agar-plates",
    type: "Lab supply",
    vendor: "Mycelium Works Lab",
    tags: ["lab"],
    categories: ["cultures", "tools"],
    description:
      "<p>Poured and sealed malt extract agar in 90 mm dishes, for isolating a clean culture from a tissue sample or checking a syringe before you trust it.</p>",
    options: { Pack: ["10 plates", "25 plates"] },
    variants: [
      v({ Pack: "10 plates" }, 18, { weight: 450 }),
      v({ Pack: "25 plates" }, 39, { weight: 1100 }),
    ],
    image: "agar-plates",
    swatch: SWATCH.gold,
    fields: {
      "custom.difficulty": "advanced",
      "custom.live_culture": false,
      "custom.substrate": ["agar"],
    },
  },

  // ----- Substrates -----
  {
    title: "Supplemented Hardwood Substrate",
    handle: "hardwood-substrate",
    type: "Substrate",
    vendor: "Mycelium Works",
    tags: ["bulk"],
    categories: ["substrates"],
    description:
      "<p>Oak sawdust with 20% soy hulls, hydrated and sterilised in a filter bag. Ready for grain spawn: the master's mix for oysters, lion's mane and shiitake.</p>",
    options: { Weight: ["2.5 kg", "5 kg"] },
    variants: [
      v({ Weight: "2.5 kg" }, 9, { weight: 2600 }),
      v({ Weight: "5 kg" }, 16, { weight: 5100 }),
    ],
    image: "hardwood-substrate",
    swatch: SWATCH.clay,
    fields: { "custom.difficulty": "intermediate", "custom.substrate": ["hardwood", "soy_hulls"] },
  },
  {
    title: "Coffee Grounds Substrate",
    handle: "coffee-substrate",
    type: "Substrate",
    vendor: "Mycelium Works",
    tags: ["upcycled"],
    categories: ["substrates"],
    description:
      "<p>Spent grounds collected from twelve local cafés the morning they're brewed, pasteurised with straw. Oysters love it, and it saves a skip of waste a week.</p>",
    price: 7,
    weight: 2100,
    image: "coffee-substrate",
    swatch: SWATCH.charcoal,
    fields: {
      "custom.difficulty": "beginner",
      "custom.substrate": ["coffee", "straw"],
      "custom.grown_in": "Our lab and farm",
      "custom.certifications": ["eu_organic"],
    },
  },

  // ----- Lab -----
  {
    title: "Pressure Sterilizer",
    handle: "pressure-sterilizer",
    type: "Equipment",
    vendor: "Sterilab",
    tags: ["equipment", "lab"],
    categories: ["sterilization"],
    description:
      "<p>A cast aluminium sterilizer that holds 121 °C at 15 psi, the temperature that kills every spore a grain jar can hide. Two gauges, a weighted valve and a gasket you'll never think about.</p>",
    options: { Capacity: ["23 L", "41 L"], Plug: ["EU", "UK", "US"] },
    // The 41 L model isn't certified for the UK.
    variants: [
      v({ Capacity: "23 L", Plug: "EU" }, 249, { weight: 7800 }),
      v({ Capacity: "23 L", Plug: "UK" }, 249, { weight: 7800 }),
      v({ Capacity: "23 L", Plug: "US" }, 249, { weight: 7800 }),
      v({ Capacity: "41 L", Plug: "EU" }, 389, { weight: 11800 }),
      v({ Capacity: "41 L", Plug: "US" }, 389, { weight: 11800 }),
    ],
    variantFields: (o) => ({
      "custom.capacity": l(o.Capacity === "23 L" ? 23 : 41),
      "custom.voltage": o.Plug === "US" ? "120v" : "230v",
    }),
    image: "pressure-sterilizer",
    swatch: SWATCH.oyster,
    fields: {
      "custom.difficulty": "intermediate",
      "custom.warranty_months": 36,
      "custom.spec_sheet": "https://myceliumworks.example/specs/pressure-sterilizer.pdf",
      "custom.works_with": ["agar-plates", "grain-spawn-bag", "hardwood-substrate"],
      "custom.certifications": ["ce"],
      "custom.supplier_code": "STL-PS-23-41",
      "custom.landed_cost": eur(118),
    },
  },
  {
    title: "Laminar Flow Hood",
    handle: "flow-hood",
    type: "Equipment",
    vendor: "Clean Air Lab",
    tags: ["equipment", "lab"],
    categories: ["clean-air"],
    description:
      "<p>A wall of filtered air pushed across your bench at 0.45 m/s, so nothing floating in the room reaches an open plate. The professional's answer to contamination.</p>",
    options: { Width: ["60 cm", "120 cm"], Plug: ["EU", "UK", "US"] },
    variants: [
      v({ Width: "60 cm", Plug: "EU" }, 690, { weight: 21000 }),
      v({ Width: "60 cm", Plug: "UK" }, 690, { weight: 21000 }),
      v({ Width: "60 cm", Plug: "US" }, 690, { weight: 21000 }),
      v({ Width: "120 cm", Plug: "EU" }, 1290, { weight: 38000 }),
      v({ Width: "120 cm", Plug: "US" }, 1290, { weight: 38000 }),
    ],
    variantFields: (o) => ({
      "custom.width": cm(o.Width === "60 cm" ? 60 : 120),
      "custom.voltage": o.Plug === "US" ? "120v" : "230v",
    }),
    image: "flow-hood",
    swatch: SWATCH.stone,
    fields: {
      "custom.difficulty": "advanced",
      "custom.warranty_months": 60,
      "custom.spec_sheet": "https://myceliumworks.example/specs/flow-hood.pdf",
      "custom.certifications": ["en_1822", "ce"],
      "custom.works_with": ["agar-plates", "inoculation-kit"],
      "custom.supplier_code": "CAL-LFH",
      "custom.landed_cost": eur(402),
    },
  },
  {
    title: "Still Air Box",
    handle: "still-air-box",
    type: "Equipment",
    vendor: "Mycelium Works",
    tags: ["equipment", "beginner"],
    categories: ["clean-air"],
    description:
      "<p>A clear tub with two arm ports. Still air carries no spores, so a wiped-down box and slow hands give you clean transfers without a flow hood.</p>",
    price: 45,
    weight: 2400,
    image: "still-air-box",
    swatch: SWATCH.pale,
    fields: {
      "custom.difficulty": "beginner",
      "custom.works_with": ["liquid-culture-syringe", "agar-plates", "inoculation-kit"],
    },
  },
  {
    title: "Inoculation Tool Kit",
    handle: "inoculation-kit",
    type: "Lab supply",
    vendor: "Mycelium Works",
    tags: ["lab"],
    categories: ["tools"],
    description:
      "<p>A scalpel with ten blades, an inoculation loop, a spirit lamp, sterile needles and a roll of Parafilm, in a canvas roll that fits in a still air box.</p>",
    price: 38,
    weight: 600,
    image: "inoculation-kit",
    swatch: SWATCH.forest,
    fields: { "custom.difficulty": "intermediate" },
  },

  // ----- Materials -----
  {
    title: "Mycelium Leather",
    handle: "mycelium-leather",
    type: "Material",
    vendor: "Mycelium Works Materials",
    tags: ["by the metre", "b2b"],
    categories: ["leather"],
    description:
      "<p>A supple sheet grown from reishi mycelium on hemp, tanned with plant extracts, sold by the running metre at 140 cm wide. It cuts, stitches and ages like leather.</p>",
    options: { Colour: ["Natural", "Charcoal", "Forest", "Clay"], Thickness: ["1.2 mm", "1.8 mm"] },
    // Forest and clay are dyed in the thin sheet only.
    variants: [
      v({ Colour: "Natural", Thickness: "1.2 mm" }, 89, { weight: 560 }),
      v({ Colour: "Natural", Thickness: "1.8 mm" }, 109, { weight: 840 }),
      v({ Colour: "Charcoal", Thickness: "1.2 mm" }, 95, { weight: 560 }),
      v({ Colour: "Charcoal", Thickness: "1.8 mm" }, 115, { weight: 840 }),
      v({ Colour: "Forest", Thickness: "1.2 mm" }, 95, { weight: 560 }),
      v({ Colour: "Clay", Thickness: "1.2 mm" }, 95, { weight: 560 }),
    ],
    variantFields: (o) => ({
      "custom.thickness": mm(o.Thickness === "1.2 mm" ? "1.2" : "1.8"),
      "custom.width": cm(140),
    }),
    imageBy: "Colour",
    image: "mycelium-leather",
    swatchBy: {
      Natural: SWATCH.cream,
      Charcoal: SWATCH.charcoal,
      Forest: SWATCH.forest,
      Clay: SWATCH.clay,
    },
    fields: {
      "custom.certifications": ["vegan", "oeko_tex"],
      "custom.substrate": ["hemp"],
      "custom.grown_in": "Our lab and farm",
      "custom.care_guide":
        "<p>Wipe with a damp cloth; condition twice a year with a plant wax. Keep it out of long, direct sun, which fades the natural shade first.</p>",
      "custom.supplier_code": "MWM-LTH",
      "custom.landed_cost": eur(31),
    },
  },
  {
    title: "Grown Packaging Inserts",
    handle: "packaging-inserts",
    type: "Material",
    vendor: "Mycelium Works Materials",
    tags: ["b2b", "bulk"],
    categories: ["packaging"],
    description:
      "<p>Protective inserts grown in moulds from hemp and mycelium in five days. As light and strong as polystyrene, and they break down in a garden compost in about six weeks.</p>",
    options: {
      Shape: ["Wine bottle", "Box insert 20 × 15 cm", "Corner protector"],
      Pack: ["50", "250"],
    },
    variants: [
      v({ Shape: "Wine bottle", Pack: "50" }, 79, { weight: 3500 }),
      v({ Shape: "Wine bottle", Pack: "250" }, 329, { weight: 17000 }),
      v({ Shape: "Box insert 20 × 15 cm", Pack: "50" }, 69, { weight: 3000 }),
      v({ Shape: "Box insert 20 × 15 cm", Pack: "250" }, 289, { weight: 15000 }),
      v({ Shape: "Corner protector", Pack: "250" }, 149, { weight: 6000 }),
    ],
    image: "packaging-inserts",
    swatch: SWATCH.cream,
    fields: {
      "custom.certifications": ["home_compostable", "vegan"],
      "custom.substrate": ["hemp"],
      "custom.grown_in": "Our lab and farm",
    },
  },
  {
    title: "Acoustic Wall Panel",
    handle: "acoustic-panel",
    type: "Material",
    vendor: "Mycelium Works Materials",
    tags: ["interiors", "b2b"],
    categories: ["acoustics"],
    description:
      "<p>Panels grown on hemp into a deep, irregular relief that scatters sound and soaks it up. Mount them with the hidden French cleat; charred ones are flame-finished for a darker, harder face.</p>",
    options: { Size: ["60 × 60 cm", "120 × 60 cm"], Finish: ["Raw", "Charred"] },
    variants: [
      v({ Size: "60 × 60 cm", Finish: "Raw" }, 69, { weight: 2200 }),
      v({ Size: "60 × 60 cm", Finish: "Charred" }, 85, { weight: 2200 }),
      v({ Size: "120 × 60 cm", Finish: "Raw" }, 125, { weight: 4300 }),
      v({ Size: "120 × 60 cm", Finish: "Charred" }, 155, { weight: 4300 }),
    ],
    variantFields: (o) => ({
      "custom.width": cm(o.Size.startsWith("120") ? 120 : 60),
      "custom.height": cm(60),
      "custom.thickness": mm(40),
    }),
    imageBy: "Finish",
    image: "acoustic-panel",
    swatchBy: { Raw: SWATCH.cream, Charred: SWATCH.charcoal },
    fields: {
      "custom.fire_rating": "b_s1_d0",
      "custom.sound_absorption": "0.75",
      "custom.certifications": ["home_compostable", "ce"],
      "custom.spec_sheet": "https://myceliumworks.example/specs/acoustic-panel.pdf",
      "custom.grown_in": "Our lab and farm",
    },
  },
  {
    title: "Grown Pendant Shade",
    handle: "pendant-shade",
    type: "Object",
    vendor: "Mycelium Works Materials",
    tags: ["interiors"],
    categories: ["objects"],
    description:
      "<p>A lampshade grown in a mould over a week, then dried and left in the texture the mycelium chose. No two match. Fits an E27 pendant set.</p>",
    options: { Shape: ["Dome", "Cone"] },
    variants: [
      v({ Shape: "Dome" }, 145, { weight: 700 }),
      v({ Shape: "Cone" }, 135, { weight: 650 }),
    ],
    variantFields: (o) => ({
      "custom.width": cm(o.Shape === "Dome" ? 42 : 36),
      "custom.height": cm(o.Shape === "Dome" ? 24 : 30),
    }),
    image: "pendant-shade",
    swatch: SWATCH.cream,
    fields: {
      "custom.certifications": ["home_compostable"],
      "custom.grown_in": "Our lab and farm",
    },
  },

  // ----- Learn -----
  {
    title: "Cultivation: From Spore to Harvest",
    handle: "cultivation-course",
    type: "Course",
    vendor: "Mycelium Works Lab",
    tags: ["online"],
    categories: ["courses"],
    description:
      "<p>Twelve lessons from our lab: sterile work, agar, liquid culture, grain spawn and fruiting, with the mistakes we made so you don't. Self-paced, or with a cohort and four live sessions.</p>",
    options: { Format: ["Self-paced", "Live cohort"] },
    variants: [
      v({ Format: "Self-paced" }, 129, { shipping: false }),
      v({ Format: "Live cohort" }, 249, { shipping: false }),
    ],
    untracked: true,
    image: "cultivation-course",
    swatch: SWATCH.lime,
    fields: {
      "custom.difficulty": "beginner",
      "custom.duration_hours": "14.5",
      "custom.next_session": "2026-11-18T18:00:00Z",
      "custom.works_with": ["still-air-box", "agar-plates", "liquid-culture-syringe"],
    },
  },
  {
    title: "The Mycelium Handbook",
    handle: "mycelium-handbook",
    type: "Book",
    vendor: "Mycelium Works Press",
    tags: ["book", "gift"],
    categories: ["books"],
    description:
      "<p>Our lab manual, rewritten for everyone: 320 pages on growing, cooking and building with fungi, with photographs from the farm.</p>",
    options: { Format: ["Paperback", "Hardcover", "eBook"] },
    variants: [
      v({ Format: "Paperback" }, 29, { weight: 780 }),
      v({ Format: "Hardcover" }, 42, { weight: 1150 }),
      // Downloaded: nothing to ship, and no stock to run out of.
      v({ Format: "eBook" }, 18, { shipping: false, untracked: true }),
    ],
    image: "mycelium-handbook",
    swatch: SWATCH.moss,
    fields: { "custom.difficulty": "beginner" },
  },

  // A draft: being tested, not for sale yet.
  {
    title: "Mycelium Building Brick",
    handle: "mycelium-brick",
    type: "Material",
    vendor: "Mycelium Works Materials",
    tags: ["prototype"],
    categories: ["objects"],
    description:
      "<p>A load-bearing brick grown from hemp and mycelium. In compression testing with the Faculty of Civil Engineering.</p>",
    price: 6,
    weight: 1300,
    draft: true,
    image: "mycelium-brick",
    swatch: SWATCH.bark,
    fields: {
      "custom.certifications": ["home_compostable"],
      "custom.supplier_code": "MWM-BRK-P1",
      "custom.landed_cost": eur(2.1),
    },
  },
];

const MARKETS = [
  {
    name: "Europe",
    currency: "EUR",
    countries: ["HR", "SI", "AT", "DE", "IT", "FR", "NL", "BE", "ES", "IE"],
    pricesIncludeTax: true,
    shipping: [
      { name: "Standard", code: "standard", flat: 490, freeFrom: 7500, days: [3, 5] },
      { name: "Express", code: "express", flat: 1290, days: [1, 2] },
    ],
  },
  {
    name: "United Kingdom",
    currency: "GBP",
    countries: ["GB"],
    pricesIncludeTax: true,
    shipping: [
      { name: "Royal Mail Tracked", code: "standard", flat: 595, freeFrom: 6000, days: [2, 4] },
    ],
  },
  {
    name: "United States",
    currency: "USD",
    countries: ["US"],
    pricesIncludeTax: false,
    shipping: [
      { name: "Ground", code: "standard", flat: 795, freeFrom: 10000, days: [4, 7] },
      { name: "2-Day", code: "express", flat: 1995, days: [2, 2] },
    ],
  },
  {
    name: "Serbia",
    currency: "RSD",
    countries: ["RS"],
    pricesIncludeTax: true,
    shipping: [
      { name: "Post Express", code: "standard", flat: 45000, freeFrom: 800000, days: [1, 3] },
    ],
  },
];

/** Standard VAT, in parts per million. */
const VAT = {
  HR: 25,
  SI: 22,
  AT: 20,
  DE: 19,
  IT: 22,
  FR: 20,
  NL: 21,
  BE: 21,
  ES: 21,
  IE: 23,
  GB: 20,
  RS: 20,
};

/** Every combination of `options`, as `{ name: value }`. */
// ---------- the run ----------

const slug = (text) =>
  text
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-|-$/g, "");

/** A SKU from the product's initials and each option value's first letters. */
function skuOf(handle, of) {
  const initials = handle
    .split("-")
    .map((w) => w[0])
    .join("")
    .toUpperCase();
  const values = Object.values(of).map((value) =>
    value
      .replace(/[^A-Za-z0-9]/g, "")
      .slice(0, 4)
      .toUpperCase(),
  );
  return [initials, ...values].join("-");
}

/** The image name an uploaded asset was seeded from: its file name without extension. */
const nameOf = (url) =>
  decodeURIComponent(
    url
      .split("/")
      .pop()
      .replace(/\.[a-z]+$/i, ""),
  );

/** Uploads the image an asset at `url` was seeded from again; the new asset's ID. */
async function again(url, alt) {
  const name = nameOf(url);
  const id = await upload(name, "#D4D5D2", alt);
  if (id) console.log(`  ${name}`);
  return id;
}

/**
 * `--images`, on a store this script already seeded: uploads each product's, variant's
 * and category's image again from `seed-assets/images` (the generated photos replacing
 * the plain placeholders), points everything at the new assets and deletes the old ones.
 * The catalogue itself is left as it is.
 */
async function replaceImages() {
  const replaced = new Map();

  console.log("Product and variant images");
  const products = await admin.get("/admin/products?limit=100");
  for (const summary of products.items) {
    const product = await admin.get(`/admin/products/${summary.id}`);
    if (product.media.length === 0) continue;
    const assets = [];
    for (const m of product.media) {
      const id = await again(m.url, m.alt ?? product.title);
      if (!id) return;
      replaced.set(m.asset_id, id);
      assets.push(id);
    }
    await admin.put(`/admin/products/${product.id}/media`, { assets });
    for (const variant of product.variants) {
      const asset = variant.media_asset_id && replaced.get(variant.media_asset_id);
      if (asset) {
        await admin.put(`/admin/products/${product.id}/variants/${variant.id}/media`, { asset });
      }
    }
  }

  console.log("Category images");
  const categories = await admin.get("/admin/categories");
  for (const top of categories.items) {
    const category = await admin.get(`/admin/categories/${top.id}`);
    if (!category.image?.url) continue;
    const id = await again(category.image.url, category.name);
    if (!id) return;
    replaced.set(category.image.asset_id ?? category.image.id, id);
    await admin.put(`/admin/categories/${top.id}/image`, { image: id });
  }

  // The placeholders are no longer used by anything.
  let removed = 0;
  for (const old of replaced.keys()) {
    if (!old) continue;
    await admin.delete(`/admin/assets/${old}`).then(
      () => removed++,
      (e) => console.warn(`  (kept ${old}: ${e.message})`),
    );
  }
  console.log(`\nDone: ${replaced.size} images replaced, ${removed} placeholders deleted.`);
}

async function main() {
  console.log(`Seeding ${BASE} as ${EMAIL}`);
  const login = await call("POST", "/admin/auth/login", { email: EMAIL, password: PASSWORD });
  if (login.status !== "authenticated")
    throw new Error(`login needs ${login.status}: use a store owner without 2FA`);
  token = login.session.token;

  if (process.argv.includes("--images")) return replaceImages();

  const existing = await admin.get("/admin/products?limit=1");
  if (existing.items.length > 0 && !process.argv.includes("--force")) {
    throw new Error("this store already has products: seed a fresh one (or pass --force)");
  }

  console.log("Store details");
  await admin.patch("/admin/store", {
    name: "Mycelium Works",
    contact_email: "hello@myceliumworks.example",
    description:
      "Grow kits, lab cultures and materials grown from mycelium, from our own lab and farm.",
    // No address or home country: the demo is deliberately nowhere in particular.
    legal_name: "Mycelium Works Ltd.",
    default_language: "en",
    setup_finished: true,
  });

  console.log("Notification settings");
  // Emails in the brand's moss green, new orders to the owner, and order links into the
  // storefront: `/order/<id>?token=` keeps the token in the buyer's cookie and opens the
  // confirmation.
  await admin.patch("/admin/notification-settings", {
    accent: "#455C01",
    sender_name: "Mycelium Works",
    reply_to: "hello@myceliumworks.example",
    footer: "Mycelium Works Ltd. · hello@myceliumworks.example",
    staff_recipients: [EMAIL],
    order_url: `${STOREFRONT_URL}/order/{order}?token={token}`,
  });

  console.log("Tax regions");
  for (const [country, percent] of Object.entries(VAT)) {
    await admin.post("/admin/tax-regions", { country, default_rate_ppm: percent * 10_000 });
  }

  console.log("Markets, catalogs and price lists");
  for (const market of MARKETS) {
    const created = await admin.post("/admin/markets", {
      name: market.name,
      countries: market.countries,
      currency: market.currency,
    });
    await admin.patch(`/admin/markets/${created.id}`, {
      prices_include_tax: market.pricesIncludeTax,
    });
    market.id = created.id;
    market.catalog =
      created.catalog ?? (await admin.post("/admin/catalogs", { market: created.id })).id;
    if (market.currency !== "EUR") {
      market.priceList = (
        await admin.post("/admin/price-lists", {
          name: `${market.name} prices`,
          currency: market.currency,
        })
      ).id;
      await admin.put(`/admin/catalogs/${market.catalog}/price-lists`, {
        price_lists: [market.priceList],
      });
    }
  }

  console.log("Warehouses");
  const byName = Object.fromEntries(MARKETS.map((m) => [m.name, m.id]));
  const farm = await admin.post("/admin/locations", {
    name: "Lab and farm",
    markets: [byName.Europe, byName["United Kingdom"], byName.Serbia],
    priority: 1,
  });
  const newJersey = await admin.post("/admin/locations", {
    name: "New Jersey fulfilment",
    markets: [byName["United States"]],
    priority: 1,
  });

  console.log("Shipping zones");
  for (const market of MARKETS) {
    const zone = await admin.post("/admin/shipping-zones", {
      name: market.name,
      countries: market.countries,
    });
    for (const option of market.shipping) {
      const tiers = option.freeFrom
        ? [{ up_to: option.freeFrom, amount: option.flat }, { amount: 0 }]
        : [{ amount: option.flat }];
      await admin.post(`/admin/shipping-zones/${zone.id}/options`, {
        name: option.name,
        code: option.code,
        rate: {
          type: "builtin",
          basis: "subtotal",
          tables: [{ currency: market.currency, tiers }],
        },
        estimate: { min_days: option.days[0], max_days: option.days[1] },
      });
    }
  }

  console.log("Custom fields");
  for (const field of FIELDS) await admin.post("/admin/custom-field-definitions", field);
  // The well-known customer fields, created from the API's suggestions as the admin's
  // "Add" would: `custom.tags` (labels) and `custom.note`.
  const suggested = await admin.get(
    "/admin/custom-field-definitions/suggestions?owner_type=customer",
  );
  for (const { why: _why, existing: taken, field: _field, ...body } of suggested.items) {
    if (!taken) await admin.post("/admin/custom-field-definitions", body);
  }
  console.log(`  ${FIELDS.length + suggested.items.length} definitions`);

  console.log("Categories");
  const categoryIds = {};
  let position = 0;
  for (const top of CATEGORIES) {
    const parent = await admin.post("/admin/categories", {
      name: top.name,
      handle: top.handle,
      visible: true,
      position: position++,
      description: `<p>${top.name} from Mycelium Works.</p>`,
    });
    categoryIds[top.handle] = parent.id;
    await admin.patch(`/admin/categories/${parent.id}/custom-fields`, {
      custom_fields: { "custom.buying_guide": top.guide },
    });
    const image = await upload(top.image, top.swatch, top.name);
    if (image) await admin.put(`/admin/categories/${parent.id}/image`, { image });
    for (const child of top.children) {
      const created = await admin.post("/admin/categories", {
        name: child.name,
        handle: child.handle,
        visible: true,
        parent: parent.id,
      });
      categoryIds[child.handle] = created.id;
    }
  }

  const channels = await admin.get("/admin/channels");
  const onlineStore = channels.items.find((c) => c.kind === "online_store");
  if (!onlineStore) throw new Error("the store has no Online Store channel");

  console.log("Products");
  const sellable = [];
  const productIds = {};
  for (const spec of PRODUCTS) {
    process.stdout.write(`  ${spec.title}`);
    const variants = spec.variants ?? [
      { of: {}, price: spec.price, weight: spec.weight, untracked: spec.untracked },
    ];

    let product = await admin.post("/admin/products", {
      handle: spec.handle,
      title: spec.title,
      price: eur(variants[0].price),
    });
    productIds[spec.handle] = product.id;
    await admin.patch(`/admin/products/${product.id}`, {
      description: spec.description,
      vendor: spec.vendor,
      product_type: spec.type,
      tags: spec.tags,
    });

    if (spec.options) {
      product = await admin.put(`/admin/products/${product.id}/options`, {
        options: Object.entries(spec.options).map(([name, values]) => ({
          name,
          values: values.map((label) => ({ label })),
        })),
      });
      const valueId = (name, label) =>
        product.options.find((o) => o.name === name).values.find((x) => x.label === label).id;
      product = await admin.put(`/admin/products/${product.id}/variants`, {
        variants: variants.map((variant) => ({
          id: null,
          sku: skuOf(spec.handle, variant.of),
          price: eur(variant.price),
          compare_at_price: null,
          selections: Object.entries(variant.of).map(([name, label]) => valueId(name, label)),
          weight_grams: variant.shipping === false ? null : variant.weight,
          requires_shipping: variant.shipping !== false,
        })),
      });
    } else {
      product = await admin.patch(
        `/admin/products/${product.id}/variants/${product.variants[0].id}`,
        { sku: skuOf(spec.handle, {}), weight_grams: spec.weight },
      );
    }
    // The stored variants line up with the spec's: both in the order given.
    const pairs = product.variants.map((stored, i) => ({ stored, spec: variants[i] }));

    for (const [i, { stored, spec: variant }] of pairs.entries()) {
      const fields = spec.variantFields?.(variant.of, i);
      if (fields)
        await admin.patch(`/admin/variants/${stored.id}/custom-fields`, { custom_fields: fields });
    }

    // Images: one per value of the option that changes how it looks, the variants
    // pointing at theirs; else one for the product.
    if (spec.imageBy) {
      const values = [...new Set(variants.map((x) => x.of[spec.imageBy]))];
      const images = {};
      for (const value of values) {
        const asset = await upload(
          `${spec.image}-${slug(value)}`,
          spec.swatchBy[value],
          `${spec.title}, ${value}`,
        );
        if (asset) images[value] = asset;
      }
      if (Object.keys(images).length > 0) {
        await admin.put(`/admin/products/${product.id}/media`, { assets: Object.values(images) });
        for (const { stored, spec: variant } of pairs) {
          const asset = images[variant.of[spec.imageBy]];
          if (asset)
            await admin.put(`/admin/products/${product.id}/variants/${stored.id}/media`, { asset });
        }
      }
    } else {
      const asset = await upload(spec.image, spec.swatch, spec.title);
      if (asset) await admin.put(`/admin/products/${product.id}/media`, { assets: [asset] });
    }

    // Stock at both warehouses, a few variants sold out; live cultures kept low, since
    // they're grown to order; nothing for what's downloaded.
    const live = spec.fields?.["custom.live_culture"] === true;
    for (const { stored, spec: variant } of pairs) {
      if (spec.untracked || variant.untracked) continue;
      const item = await admin.post("/admin/inventory-items", {
        variant: stored.id,
        tracked: true,
      });
      const soldOut = random() < 0.1;
      const most = live ? 15 : 60;
      await admin.put(`/admin/inventory-items/${item.id}/levels/${farm.id}`, {
        on_hand: soldOut ? 0 : 3 + Math.floor(random() * most),
      });
      await admin.put(`/admin/inventory-items/${item.id}/levels/${newJersey.id}`, {
        on_hand: soldOut || live ? 0 : Math.floor(random() * 20),
      });
    }

    // Prices in the other currencies.
    for (const market of MARKETS.filter((m) => m.priceList)) {
      await admin.put(`/admin/price-lists/${market.priceList}/fixed-prices`, {
        fixed_prices: product.variants.map((x) => ({
          variant: x.id,
          price: convert(x.price.amount, market.currency),
        })),
      });
    }

    for (const handle of spec.categories) {
      await admin.post(`/admin/categories/${categoryIds[handle]}/products`, {
        products: [product.id],
      });
    }
    if (!spec.draft) {
      await admin.patch(`/admin/products/${product.id}/status`, { status: "active" });
      sellable.push({ ...spec, id: product.id, variants: product.variants });
    }
    process.stdout.write(` · ${product.variants.length} variant(s)\n`);
  }

  // Product fields last: "Works with" references products made after the one holding it.
  console.log("Product fields");
  for (const spec of PRODUCTS) {
    if (!spec.fields) continue;
    const fields = { ...spec.fields };
    if (fields["custom.works_with"])
      fields["custom.works_with"] = fields["custom.works_with"].map((handle) => {
        const id = productIds[handle];
        if (!id) throw new Error(`${spec.handle} works with an unknown product, ${handle}`);
        return id;
      });
    await admin.patch(`/admin/products/${productIds[spec.handle]}/custom-fields`, {
      custom_fields: fields,
    });
  }

  const ids = sellable.map((p) => p.id);
  for (const market of MARKETS)
    await admin.post(`/admin/catalogs/${market.catalog}/products`, { products: ids });
  await admin.post(`/admin/channels/${onlineStore.id}/products`, { products: ids });

  console.log("Sales and clearance");
  // In euros, the Europe market's base prices: its catalog takes the lists.
  const europe = MARKETS.find((m) => m.currency === "EUR");
  const forSale = Object.fromEntries(sellable.map((p) => [p.handle, p]));
  // A "was" price on a few variants' own prices, struck through wherever no list wins.
  for (const handle of ["fruiting-chamber", "pressure-sterilizer", "still-air-box"]) {
    const product = forSale[handle];
    if (!product) continue;
    const variant = product.variants[0];
    await admin.patch(`/admin/products/${product.id}/variants/${variant.id}`, {
      compare_at_price: share(variant.price, 1.25),
    });
  }
  // A season's sale: 20% off every grow kit, started yesterday, ending in a month. A sale
  // list's "was" price is the regular one, so nothing else needs typing.
  const day = 24 * 60 * 60 * 1000;
  const autumn = await admin.post("/admin/price-lists", {
    name: "Autumn sale",
    description: "20% off grow kits while the season lasts.",
    kind: "sale",
    currency: "EUR",
    starts_at: new Date(Date.now() - day).toISOString(),
    ends_at: new Date(Date.now() + 30 * day).toISOString(),
    adjustment: {
      percent_bps: -2000,
      applies_to: { type: "categories", categories: [categoryIds["grow-kits"]] },
    },
  });
  await admin.post(`/admin/catalogs/${europe.catalog}/price-lists`, { price_list: autumn.id });
  // Clearance: the last of a few lines at fixed prices, the regular price as the "was",
  // with no end.
  const clearance = await admin.post("/admin/price-lists", {
    name: "Clearance",
    description: "The last of the old stock.",
    kind: "sale",
    currency: "EUR",
  });
  const CLEARANCE = { "acoustic-panel": 0.6, "mycelium-brick": 0.5, "packaging-inserts": 0.7 };
  await admin.put(`/admin/price-lists/${clearance.id}/fixed-prices`, {
    fixed_prices: Object.entries(CLEARANCE).flatMap(([handle, factor]) =>
      (forSale[handle]?.variants ?? []).slice(0, 2).map((variant) => ({
        variant: variant.id,
        price: share(variant.price, factor),
        compare_at: variant.price,
      })),
    ),
  });
  await admin.post(`/admin/catalogs/${europe.catalog}/price-lists`, {
    price_list: clearance.id,
  });

  console.log("Search");
  await admin.patch("/admin/search/settings", { split_by: [], facets: FACETS });

  console.log("Publishable key and the storefront's webhook");
  const key = await admin.post("/admin/api-keys", {
    kind: "publishable",
    name: "Storefront",
    channel: onlineStore.id,
  });
  let webhookSecret;
  try {
    const app = await admin.post("/admin/apps", {
      key: "app_storefront",
      name: "Storefront",
      subscriptions: [
        "product.*",
        "inventory_item.*",
        "price_list.*",
        "catalog.*",
        "channel.*",
        "category.*",
        "market.*",
        "custom_field_definition.*",
      ],
      webhook: { url: `${STOREFRONT_URL}/api/revalidate` },
    });
    webhookSecret = app.secret;
  } catch (e) {
    console.warn(`  (no webhook: ${e.message})`);
  }

  console.log("Payment methods");
  // Manual ones: the shopper is told how to pay, staff mark it paid.
  const cashOnDelivery = await admin.post("/admin/payment-methods", {
    provider: "manual",
    name: "Cash on delivery",
    instructions: "Pay the courier in cash when your order arrives.",
    markets: [byName.Serbia],
  });
  const bankTransfer = await admin.post("/admin/payment-methods", {
    provider: "manual",
    name: "Bank transfer",
    instructions: [
      "Transfer the total to:",
      "Mycelium Works Ltd., Demo Bank",
      "IBAN: XX00 DEMO 0000 0000 0000 00",
      "BIC: DEMOXX2X",
      "Use your order number as the reference. We ship once the payment arrives.",
    ].join("\n"),
  });

  console.log("Customers and orders");
  const SHOPPERS = [
    {
      email: "ana.kovac@example.com",
      first: "Ana",
      last: "Kovač",
      country: "SI",
      line1: "Trubarjeva cesta 12",
      city: "Ljubljana",
      postal: "1000",
    },
    {
      email: "lukas.weber@example.com",
      first: "Lukas",
      last: "Weber",
      country: "DE",
      line1: "Torstraße 140",
      city: "Berlin",
      postal: "10119",
    },
    {
      email: "chloe.martin@example.com",
      first: "Chloé",
      last: "Martin",
      country: "FR",
      line1: "12 Rue de Bretagne",
      city: "Paris",
      postal: "75003",
    },
    {
      email: "oliver.smith@example.com",
      first: "Oliver",
      last: "Smith",
      country: "GB",
      line1: "21 Columbia Road",
      city: "London",
      postal: "E2 7RG",
    },
    {
      email: "emma.johnson@example.com",
      first: "Emma",
      last: "Johnson",
      country: "US",
      line1: "88 Grand Street",
      city: "New York",
      postal: "10013",
      province: "NY",
    },
    {
      email: "marko.petrovic@example.com",
      first: "Marko",
      last: "Petrović",
      country: "RS",
      line1: "Knez Mihailova 30",
      city: "Beograd",
      postal: "11000",
    },
    {
      email: "sofia.rossi@example.com",
      first: "Sofia",
      last: "Rossi",
      country: "IT",
      line1: "Via Tortona 27",
      city: "Milano",
      postal: "20144",
    },
    {
      email: "jan.devries@example.com",
      first: "Jan",
      last: "de Vries",
      country: "NL",
      line1: "Prinsengracht 263",
      city: "Amsterdam",
      postal: "1016 GV",
    },
  ];
  const liveProducts = new Set(
    sellable.filter((p) => p.fields?.["custom.live_culture"] === true).map((p) => p.id),
  );
  // What ships, by variant: order lines don't say.
  const ships = new Map(
    sellable.flatMap((p) => p.variants.map((x) => [x.id, x.requires_shipping !== false])),
  );
  const orders = [];
  for (const shopper of SHOPPERS) {
    const sf = (method, path, body) => call(method, path, body, { key: key.token });
    const cart = await sf("POST", "/storefront/carts", { country: shopper.country });
    const lines = 1 + Math.floor(random() * 3);
    let live = false;
    for (let i = 0; i < lines; i++) {
      const product = pick(sellable);
      const variant = pick(product.variants);
      try {
        await sf("POST", `/storefront/carts/${cart.id}/lines`, {
          variant: variant.id,
          quantity: 1 + Math.floor(random() * 2),
        });
        if (liveProducts.has(product.id)) live = true;
      } catch (e) {
        if (e.code !== "out_of_stock") throw e; // a sold-out pick: skip it
      }
    }
    await sf("PUT", `/storefront/carts/${cart.id}/email`, { email: shopper.email });
    await sf("PUT", `/storefront/carts/${cart.id}/shipping-address`, {
      address: {
        first_name: shopper.first,
        last_name: shopper.last,
        line1: shopper.line1,
        city: shopper.city,
        postal_code: shopper.postal,
        province: shopper.province ?? null,
        country: shopper.country,
      },
    });
    const offers = await sf("GET", `/storefront/carts/${cart.id}/shipping-options`);
    for (const group of offers.groups) {
      const offer = group.offers.find((o) => o.option) ?? group.offers[0];
      if (offer?.option)
        await sf("PUT", `/storefront/carts/${cart.id}/shipping/${group.group}`, {
          option: offer.option,
        });
    }
    let priced = await sf("GET", `/storefront/carts/${cart.id}`);
    if (priced.lines.length === 0) continue;
    // Checkout needs a method when there's something to pay: cash on delivery where it's
    // offered (Serbia), else a bank transfer.
    if (priced.total.amount > 0) {
      const methods = await sf("GET", `/storefront/carts/${cart.id}/payment-methods`);
      const method =
        methods.items.find((m) => m.id === cashOnDelivery.id) ??
        methods.items.find((m) => m.id === bankTransfer.id);
      priced = await sf("PUT", `/storefront/carts/${cart.id}/payment-method`, {
        payment_method: method.id,
      });
    }
    const order = await call(
      "POST",
      `/storefront/carts/${cart.id}/checkout`,
      { expected_total: priced.total },
      {
        key: key.token,
        headers: { "idempotency-key": `seed-${cart.id}` },
      },
    );
    orders.push({ order, shopper, live });
    const paying = order.payment?.method_name ? `, ${order.payment.method_name}` : "";
    console.log(
      `  #${order.number} ${shopper.first} ${shopper.last} (${shopper.country}) ${(priced.total.amount / 100).toFixed(2)} ${priced.total.currency}${paying}`,
    );
  }

  // What the warehouse needs: a cold pack for anything alive, and one gift message.
  for (const [i, { order, live }] of orders.entries()) {
    const fields = {
      ...(live ? { "custom.cold_pack": true } : {}),
      ...(i === 1
        ? { "custom.gift_message": "Happy birthday, Mira! Your own lion's mane farm. — Lukas" }
        : {}),
    };
    if (Object.keys(fields).length > 0)
      await admin.patch(`/admin/orders/${order.id}/custom-fields`, { custom_fields: fields });
  }

  // Their states: paid, paid and shipped with tracking, one cancelled (its pending
  // payment voided), the rest awaiting payment. A paid order carries what staff have to
  // show for it: the bank's reference for a transfer, the courier's receipt for cash.
  for (const [i, { order, shopper }] of orders.entries()) {
    if (i === orders.length - 1) {
      await admin.post(`/admin/orders/${order.id}/cancel`, { reason: "customer", notify: false });
      continue;
    }
    if (i % 3 === 2) continue; // left pending
    const cash = order.payment?.method_name === "Cash on delivery";
    await admin.post(`/admin/orders/${order.id}/paid`, {
      reference: cash ? `Courier receipt PE-${4100 + i}` : `TRF-${order.number}-${20261000 + i}`,
    });
    // Shipped: only what ships (a course or an eBook is never fulfilled from a shelf).
    const shippable = order.lines.filter((line) => ships.get(line.variant) !== false);
    if (i % 2 === 0 && shippable.length > 0) {
      const location = shopper.country === "US" ? newJersey.id : farm.id;
      await admin.post(`/admin/orders/${order.id}/fulfillments`, {
        location,
        lines: shippable.map((line) => ({ line: line.id, quantity: line.quantity })),
        tracking: [
          {
            carrier: shopper.country === "US" ? "UPS" : "DHL",
            number: `JD01460000${1000 + i}`,
            url: null,
          },
        ],
        notify: false,
      });
    }
  }

  console.log("Customer fields and groups");
  const wholesale = await admin.post("/admin/customer-groups", {
    name: "Wholesale",
    handle: "wholesale",
    description: "Farms, labs and studios buying at trade prices.",
  });
  const CUSTOMERS = {
    "ana.horvat@example.com": {
      "custom.business_type": "home",
      "custom.tags": ["vip", "newsletter"],
      "custom.note": "Grows on her balcony; asked for a reishi workshop.",
    },
    "lukas.weber@example.com": {
      "custom.business_type": "studio",
      "custom.vat_id": "DE123456789",
      "custom.tags": ["trade"],
      "custom.note": "Buys mycelium leather for his bag studio; invoices with his VAT ID.",
    },
    "chloe.martin@example.com": { "custom.business_type": "restaurant", "custom.tags": ["trade"] },
    "marko.petrovic@example.com": { "custom.business_type": "farm", "custom.tags": ["newsletter"] },
    "sofia.rossi@example.com": {
      "custom.business_type": "lab",
      "custom.vat_id": "IT12345678901",
      "custom.tags": ["vip", "trade"],
      "custom.note":
        "University lab; a syringe arrived contaminated in 2026-09, replaced the same week.",
    },
  };
  for (const [email, fields] of Object.entries(CUSTOMERS)) {
    const found = await admin.get(`/admin/customers?q=${encodeURIComponent(email)}`);
    const customer = found.items.find((c) => c.email === email);
    if (!customer) continue;
    await admin.patch(`/admin/customers/${customer.id}/custom-fields`, { custom_fields: fields });
    if (fields["custom.tags"].includes("trade")) {
      await admin.post(`/admin/customer-groups/${wholesale.id}/customers`, {
        customers: [customer.id],
      });
    }
  }

  // Their prices: fixed below the regular ones on what they buy in bulk, cheaper again by
  // the ten and the fifty, seen once they sign in (and in their carts).
  console.log("Wholesale prices");
  const tradePrices = await admin.post("/admin/price-lists", {
    name: "Wholesale",
    description: "Trade prices and quantity breaks for the Wholesale group.",
    kind: "override",
    currency: "EUR",
    audience: { groups: [wholesale.id] },
  });
  const BULK = ["mycelium-leather", "grain-spawn-bag", "hardwood-substrate", "coffee-substrate"];
  await admin.put(`/admin/price-lists/${tradePrices.id}/fixed-prices`, {
    fixed_prices: BULK.flatMap((handle) =>
      (forSale[handle]?.variants ?? []).map((variant) => ({
        variant: variant.id,
        price: share(variant.price, 0.85),
        breaks: [
          { min_quantity: 10, price: share(variant.price, 0.75) },
          { min_quantity: 50, price: share(variant.price, 0.65) },
        ],
      })),
    ),
  });
  await admin.post(`/admin/catalogs/${europe.catalog}/price-lists`, {
    price_list: tradePrices.id,
  });

  const variantCount = sellable.reduce((n, p) => n + p.variants.length, 0);
  console.log("\nDone: Mycelium Works is set up.");
  console.log(
    `  ${sellable.length} products for sale (${variantCount} variants), 1 draft · ${FIELDS.length} custom fields · ${MARKETS.length} markets · ${orders.length} orders`,
  );
  console.log("\nFor the storefront (apps/storefront/.dev.vars):");
  console.log(`  STOREFRONT_KEY=${key.token}`);
  if (webhookSecret) console.log(`  HYPHA_WEBHOOK_SECRET=${webhookSecret}`);
}

main().catch((e) => {
  console.error(`\nSeeding failed: ${e.message}`);
  process.exit(1);
});
