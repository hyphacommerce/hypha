#!/usr/bin/env node
// A demo store for the admin and the storefront: "Lumen & Loom", clothing and homeware.
// It goes through the admin and storefront APIs like a merchant and their shoppers
// would, so every rule applies: store details, four markets (EUR, GBP, USD, RSD) with
// tax regions, price lists and fixed prices, two warehouses, shipping zones with rates,
// a category tree, products with sizes and colours (sparse variants, generated colour
// images, stock, some sold out), publishing, search filters, a publishable key, the
// storefront's webhook, payment methods (cash on delivery in Serbia, bank transfer
// everywhere), and a few orders in different states, some paid.
//
// Run it against a fresh store (it stops when products already exist):
//
//   HYPHA_DEV_DB=hypha_demo HYPHA_OWNER_EMAIL=owner@example.com \
//     HYPHA_OWNER_PASSWORD=demo-owner-password pnpm dev
//   pnpm seed                     # in another terminal
//
// HYPHA_URL (default http://127.0.0.1:3917), HYPHA_SEED_EMAIL and HYPHA_SEED_PASSWORD
// (default the owner above) say where and as whom. Images need the API's file storage
// (the compose S3 server under `cargo run`); without it products get none.

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

/** Uploads a colour's image (once per colour and product); `null` without file storage. */
async function upload(filename, hex, alt) {
  if (!mediaWorks) return null;
  const file = productImage(hex);
  let created;
  try {
    created = await admin.post("/admin/assets", {
      filename,
      mime_type: "image/png",
      byte_size: file.length,
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
  const stored = await fetch(created.upload.url, { method: "PUT", headers, body: file });
  if (!stored.ok) throw new Error(`uploading ${filename} to storage failed: ${stored.status}`);
  const asset = await admin.post(`/admin/assets/${created.asset.id}/complete`);
  await admin.patch(`/admin/assets/${asset.id}`, { alt });
  return asset.id;
}

// ---------- the catalogue ----------

const COLORS = {
  White: "#f2efe9",
  Black: "#24221f",
  Sage: "#9aa58a",
  Oatmeal: "#d9cbb2",
  Navy: "#2c3a52",
  Charcoal: "#4a4846",
  "Sky blue": "#a9c4dc",
  Olive: "#626a3e",
  Khaki: "#b9a77f",
  Camel: "#b88a58",
  Grey: "#9a9894",
  Herringbone: "#7d746a",
  Natural: "#e6dccb",
  Cognac: "#8a4f2a",
  Speckled: "#e3ddd1",
  Ink: "#2f3d4f",
  Clay: "#b5714f",
  Stone: "#bdb5a6",
  Mustard: "#c99a2e",
  Terracotta: "#c1673f",
  Rust: "#a4502a",
};

/**
 * Each product: its options (the first is the one images follow when it's Color), the
 * combinations sold (sparse: not every size in every colour), a base price per variant,
 * and stock (`null` untracked; 0 sold out).
 */
const PRODUCTS = [
  {
    title: "Organic Cotton Tee",
    handle: "organic-cotton-tee",
    vendor: "Lumen & Loom",
    type: "T-shirt",
    tags: ["organic", "essentials", "summer"],
    categories: ["tops"],
    description:
      "<p>A heavyweight tee in GOTS-certified organic cotton, garment-dyed for a soft, lived-in hand.</p><ul><li>220 g/m² single jersey</li><li>Relaxed fit, dropped shoulder</li><li>Made in Portugal</li></ul>",
    options: { Color: ["White", "Black", "Sage"], Size: ["XS", "S", "M", "L", "XL"] },
    price: 29,
    weight: 220,
  },
  {
    title: "Merino Crew Sweater",
    handle: "merino-crew-sweater",
    vendor: "Nordvik",
    type: "Sweater",
    tags: ["merino", "knitwear", "winter"],
    categories: ["knitwear"],
    description:
      "<p>Extra-fine 18.5 micron merino, knitted seamless on a 12-gauge machine. Warm without the bulk, and it doesn't itch.</p><p>Hand wash cold, dry flat.</p>",
    options: { Color: ["Oatmeal", "Navy", "Charcoal"], Size: ["S", "M", "L", "XL"] },
    price: 119,
    weight: 380,
  },
  {
    title: "Linen Button-Down Shirt",
    handle: "linen-button-down",
    vendor: "Atelier Sade",
    type: "Shirt",
    tags: ["linen", "summer"],
    categories: ["tops"],
    description:
      "<p>Washed European flax linen with corozo buttons and a soft-roll collar. Gets better with every wash.</p>",
    options: { Color: ["White", "Sky blue"], Size: ["S", "M", "L", "XL"] },
    price: 79,
    weight: 260,
  },
  {
    title: "Waxed Field Jacket",
    handle: "waxed-field-jacket",
    vendor: "Kestrel Goods",
    type: "Jacket",
    tags: ["outerwear", "waxed cotton"],
    categories: ["outerwear"],
    description:
      "<p>British waxed cotton over a cotton twill lining, with four bellows pockets and a corduroy collar. Re-proof once a season and it lasts decades.</p>",
    options: { Color: ["Olive", "Navy"], Size: ["S", "M", "L", "XL"] },
    // Navy only in the middle sizes: variants are sparse.
    sold: (o) => o.Color === "Olive" || ["M", "L"].includes(o.Size),
    price: 249,
    weight: 1400,
  },
  {
    title: "Relaxed Chino",
    handle: "relaxed-chino",
    vendor: "Lumen & Loom",
    type: "Trousers",
    tags: ["essentials"],
    categories: ["bottoms"],
    description:
      "<p>Brushed cotton twill with a relaxed, tapered leg and a hidden coin pocket.</p>",
    options: { Color: ["Khaki", "Navy", "Black"], Waist: ["28", "30", "32", "34", "36"] },
    price: 89,
    weight: 560,
  },
  {
    title: "Cashmere Beanie",
    handle: "cashmere-beanie",
    vendor: "Nordvik",
    type: "Hat",
    tags: ["cashmere", "winter", "gift"],
    categories: ["accessories"],
    description: "<p>Two-ply Mongolian cashmere in a ribbed knit, with a fold-up cuff.</p>",
    options: { Color: ["Camel", "Grey", "Black"] },
    price: 65,
    weight: 90,
  },
  {
    title: "Herringbone Wool Scarf",
    handle: "herringbone-wool-scarf",
    vendor: "Kestrel Goods",
    type: "Scarf",
    tags: ["wool", "winter", "gift"],
    categories: ["scarves"],
    description:
      "<p>Woven in a Scottish mill from lambswool, with hand-twisted fringes. 180 × 30 cm.</p>",
    options: { Color: ["Herringbone", "Charcoal"] },
    price: 79,
    weight: 180,
  },
  {
    title: "Canvas Tote",
    handle: "canvas-tote",
    vendor: "Lumen & Loom",
    type: "Bag",
    tags: ["everyday", "gift"],
    categories: ["bags"],
    description:
      "<p>16 oz cotton canvas with leather handles and an inside zip pocket. Fits a laptop.</p>",
    options: { Color: ["Natural", "Black"] },
    price: 45,
    weight: 450,
  },
  {
    title: "Leather Weekender",
    handle: "leather-weekender",
    vendor: "Atelier Sade",
    type: "Bag",
    tags: ["leather", "travel"],
    categories: ["bags"],
    description:
      "<p>Vegetable-tanned full-grain leather, solid brass hardware and a detachable shoulder strap. 45 litres: cabin size on most airlines.</p>",
    options: { Color: ["Cognac", "Black"] },
    price: 329,
    weight: 2100,
  },
  {
    title: "Stoneware Mug",
    handle: "stoneware-mug",
    vendor: "Clayworks Studio",
    type: "Mug",
    tags: ["ceramics", "gift"],
    categories: ["kitchen"],
    description:
      "<p>Wheel-thrown stoneware with a reactive glaze, so no two are alike. 350 ml, dishwasher safe.</p>",
    options: { Color: ["Speckled", "Ink", "Clay"] },
    price: 22,
    weight: 420,
  },
  {
    title: "Linen Tea Towel Set",
    handle: "linen-tea-towels",
    vendor: "Lumen & Loom",
    type: "Tea towel",
    tags: ["linen", "kitchen"],
    categories: ["kitchen"],
    description:
      "<p>Two stonewashed linen tea towels, 50 × 70 cm. Lint-free and very absorbent.</p>",
    color: "Natural",
    price: 28,
    weight: 260,
  },
  {
    title: "Cast Iron Skillet",
    handle: "cast-iron-skillet",
    vendor: "Kestrel Goods",
    type: "Cookware",
    tags: ["cookware"],
    categories: ["kitchen"],
    description: "<p>Pre-seasoned cast iron, ready to use. Oven, grill and induction safe.</p>",
    color: "Black",
    options: { Size: ["20 cm", "26 cm", "30 cm"] },
    priceOf: (o) => ({ "20 cm": 49, "26 cm": 69, "30 cm": 89 })[o.Size],
    weightOf: (o) => ({ "20 cm": 1800, "26 cm": 2700, "30 cm": 3600 })[o.Size],
  },
  {
    title: "Washed Linen Duvet Cover",
    handle: "washed-linen-duvet-cover",
    vendor: "Lumen & Loom",
    type: "Duvet cover",
    tags: ["linen", "bedding"],
    categories: ["bedding"],
    description:
      "<p>Garment-washed linen with coconut buttons. Breathable in summer, warm in winter.</p>",
    options: { Color: ["Natural", "Stone", "Rust"], Size: ["Single", "Double", "King"] },
    priceOf: (o) => ({ Single: 149, Double: 179, King: 199 })[o.Size],
    weight: 1300,
  },
  {
    title: "Percale Pillowcases (Pair)",
    handle: "percale-pillowcases",
    vendor: "Lumen & Loom",
    type: "Pillowcase",
    tags: ["cotton", "bedding"],
    categories: ["bedding"],
    description: "<p>Crisp, cool 300 thread count cotton percale with an envelope closure.</p>",
    color: "White",
    options: { Size: ["Standard", "King"] },
    priceOf: (o) => ({ Standard: 39, King: 45 })[o.Size],
    weight: 400,
  },
  {
    title: "Wool Throw Blanket",
    handle: "wool-throw",
    vendor: "Nordvik",
    type: "Throw",
    tags: ["wool", "home", "gift"],
    categories: ["decor"],
    description: "<p>Woven from undyed Norwegian wool. 130 × 180 cm.</p>",
    options: { Color: ["Mustard", "Grey"] },
    price: 129,
    weight: 1100,
  },
  {
    title: "Ceramic Vase",
    handle: "ceramic-vase",
    vendor: "Clayworks Studio",
    type: "Vase",
    tags: ["ceramics", "home"],
    categories: ["decor"],
    description: "<p>A matte, hand-built vase. Watertight, for fresh or dried stems.</p>",
    options: { Color: ["White", "Terracotta"], Size: ["Small", "Large"] },
    priceOf: (o) => ({ Small: 39, Large: 59 })[o.Size],
    weightOf: (o) => ({ Small: 700, Large: 1500 })[o.Size],
  },
  {
    title: "Beeswax Candle",
    handle: "beeswax-candle",
    vendor: "Clayworks Studio",
    type: "Candle",
    tags: ["home", "gift"],
    categories: ["decor"],
    description:
      "<p>Pure beeswax in a reusable ceramic pot. About 40 hours of clean, honey-scented burn.</p>",
    color: "Mustard",
    price: 18,
    weight: 300,
    untracked: true,
  },
  {
    title: "Field Notebooks (3-Pack)",
    handle: "field-notebooks",
    vendor: "Kestrel Goods",
    type: "Notebook",
    tags: ["stationery", "gift"],
    categories: ["accessories"],
    description: "<p>Pocket notebooks with recycled paper and a dot grid. 48 pages each.</p>",
    color: "Khaki",
    price: 14,
    weight: 150,
  },
  {
    title: "Alpaca Cardigan",
    handle: "alpaca-cardigan",
    vendor: "Nordvik",
    type: "Cardigan",
    tags: ["alpaca", "knitwear"],
    categories: ["knitwear"],
    description: "<p>Coming next season: brushed baby alpaca in a relaxed shawl collar.</p>",
    options: { Color: ["Camel", "Grey"], Size: ["S", "M", "L"] },
    price: 189,
    weight: 450,
    draft: true,
  },
];

const CATEGORIES = [
  {
    name: "Clothing",
    handle: "clothing",
    children: [
      { name: "Tops", handle: "tops" },
      { name: "Knitwear", handle: "knitwear" },
      { name: "Outerwear", handle: "outerwear" },
      { name: "Bottoms", handle: "bottoms" },
    ],
  },
  {
    name: "Home",
    handle: "home",
    children: [
      { name: "Kitchen", handle: "kitchen" },
      { name: "Bedding", handle: "bedding" },
      { name: "Decor", handle: "decor" },
    ],
  },
  {
    name: "Accessories",
    handle: "accessories",
    children: [
      { name: "Bags", handle: "bags" },
      { name: "Scarves", handle: "scarves" },
    ],
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
function combinations(options) {
  return Object.entries(options).reduce(
    (all, [name, values]) => all.flatMap((c) => values.map((v) => ({ ...c, [name]: v }))),
    [{}],
  );
}

// ---------- the run ----------

async function main() {
  console.log(`Seeding ${BASE} as ${EMAIL}`);
  const login = await call("POST", "/admin/auth/login", { email: EMAIL, password: PASSWORD });
  if (login.status !== "authenticated")
    throw new Error(`login needs ${login.status}: use a store owner without 2FA`);
  token = login.session.token;

  const existing = await admin.get("/admin/products?limit=1");
  if (existing.items.length > 0 && !process.argv.includes("--force")) {
    throw new Error("this store already has products: seed a fresh one (or pass --force)");
  }

  console.log("Store details");
  await admin.patch("/admin/store", {
    name: "Lumen & Loom",
    contact_email: "hello@lumenandloom.example",
    description: "Considered essentials for your wardrobe and home, made to last.",
    legal_name: "Lumen & Loom d.o.o.",
    legal_address: {
      last_name: "Lumen & Loom d.o.o.",
      company: "Lumen & Loom d.o.o.",
      line1: "Ilica 42",
      city: "Zagreb",
      postal_code: "10000",
      country: "HR",
      phone: "+385 1 4800 100",
    },
    timezone: "Europe/Zagreb",
    default_language: "en",
    setup_finished: true,
  });

  console.log("Notification settings");
  // Emails in the shop's colours (clay 700, the theme's accent), new orders to the owner,
  // and order links into the storefront: `/order/<id>?token=` keeps the token in the
  // buyer's cookie and opens the confirmation (design note 0028, §7).
  await admin.patch("/admin/notification-settings", {
    accent: "#953f21",
    sender_name: "Lumen & Loom",
    reply_to: "hello@lumenandloom.example",
    footer: "Lumen & Loom d.o.o. · Ilica 42, 10000 Zagreb, Croatia · hello@lumenandloom.example",
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
      market.priceList = (await admin.post("/admin/price-lists", { currency: market.currency })).id;
      await admin.put(`/admin/catalogs/${market.catalog}/price-list`, {
        price_list: market.priceList,
      });
    }
  }

  console.log("Warehouses");
  const byName = Object.fromEntries(MARKETS.map((m) => [m.name, m.id]));
  const zagreb = await admin.post("/admin/locations", {
    name: "Zagreb warehouse",
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

  console.log("Categories");
  const categoryIds = {};
  let position = 0;
  for (const top of CATEGORIES) {
    const parent = await admin.post("/admin/categories", {
      name: top.name,
      handle: top.handle,
      visible: true,
      position: position++,
      description: `<p>${top.name} from Lumen &amp; Loom and the makers we work with.</p>`,
    });
    categoryIds[top.handle] = parent.id;
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
  for (const spec of PRODUCTS) {
    process.stdout.write(`  ${spec.title}`);
    const options = spec.options ?? {};
    const combos = combinations(options).filter((o) => (spec.sold ? spec.sold(o) : true));
    const priceOf = (o) => eur(spec.priceOf ? spec.priceOf(o) : spec.price);
    const weightOf = (o) => (spec.weightOf ? spec.weightOf(o) : spec.weight);

    let product = await admin.post("/admin/products", {
      handle: spec.handle,
      title: spec.title,
      price: priceOf(combos[0]),
    });
    await admin.patch(`/admin/products/${product.id}`, {
      description: spec.description,
      vendor: spec.vendor,
      product_type: spec.type,
      tags: spec.tags,
    });

    if (Object.keys(options).length > 0) {
      product = await admin.put(`/admin/products/${product.id}/options`, {
        options: Object.entries(options).map(([name, values]) => ({
          name,
          values: values.map((label) => ({ label })),
        })),
      });
      const valueId = (name, label) =>
        product.options.find((o) => o.name === name).values.find((v) => v.label === label).id;
      const sku = (o) =>
        [
          spec.handle
            .split("-")
            .map((w) => w[0])
            .join("")
            .toUpperCase(),
          ...Object.values(o).map((v) =>
            v
              .replace(/[^A-Za-z0-9]/g, "")
              .slice(0, 4)
              .toUpperCase(),
          ),
        ].join("-");
      product = await admin.put(`/admin/products/${product.id}/variants`, {
        variants: combos.map((o) => ({
          id: null,
          sku: sku(o),
          price: priceOf(o),
          selections: Object.entries(o).map(([name, label]) => valueId(name, label)),
          weight_grams: weightOf(o),
          requires_shipping: true,
        })),
      });
    } else {
      const variant = product.variants[0];
      product = await admin.patch(`/admin/products/${product.id}/variants/${variant.id}`, {
        sku: spec.handle.toUpperCase().replace(/-/g, "").slice(0, 10),
        weight_grams: spec.weight,
      });
    }

    // One image per colour (or the product's one colour), the variants pointing at theirs.
    const colourOf = (variant) => {
      const selected = variant.selections?.map((id) =>
        product.options
          .flatMap((o) => o.values.map((v) => ({ name: o.name, ...v })))
          .find((v) => v.id === id),
      );
      return selected?.find((v) => v?.name === "Color")?.label ?? spec.color;
    };
    const colours = [...new Set(product.variants.map(colourOf).filter(Boolean))];
    const images = {};
    for (const colour of colours) {
      const asset = await upload(
        `${spec.handle}-${colour.toLowerCase().replace(/ /g, "-")}.png`,
        COLORS[colour],
        `${spec.title} in ${colour}`,
      );
      if (asset) images[colour] = asset;
    }
    if (Object.keys(images).length > 0) {
      await admin.put(`/admin/products/${product.id}/media`, { assets: Object.values(images) });
      if (colours.length > 1) {
        for (const variant of product.variants) {
          const asset = images[colourOf(variant)];
          if (asset)
            await admin.put(`/admin/products/${product.id}/variants/${variant.id}/media`, {
              asset,
            });
        }
      }
    }

    // Stock: tracked at both warehouses, a few variants sold out; candles untracked.
    for (const variant of product.variants) {
      if (spec.untracked) continue;
      const item = await admin.post("/admin/inventory-items", {
        variant: variant.id,
        tracked: true,
      });
      const soldOut = random() < 0.12;
      await admin.put(`/admin/inventory-items/${item.id}/levels/${zagreb.id}`, {
        on_hand: soldOut ? 0 : 5 + Math.floor(random() * 60),
      });
      await admin.put(`/admin/inventory-items/${item.id}/levels/${newJersey.id}`, {
        on_hand: soldOut ? 0 : Math.floor(random() * 25),
      });
    }

    // Prices in the other currencies.
    for (const market of MARKETS.filter((m) => m.priceList)) {
      await admin.put(`/admin/price-lists/${market.priceList}/fixed-prices`, {
        fixed_prices: product.variants.map((v) => ({
          variant: v.id,
          price: convert(v.price.amount, market.currency),
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

  const ids = sellable.map((p) => p.id);
  for (const market of MARKETS)
    await admin.post(`/admin/catalogs/${market.catalog}/products`, { products: ids });
  await admin.post(`/admin/channels/${onlineStore.id}/products`, { products: ids });

  console.log("Search");
  await admin.patch("/admin/search/settings", {
    split_by: [],
    facets: [
      { key: "option.color", label: "Colour" },
      { key: "option.size", label: "Size" },
      { key: "product_type", label: "Type" },
      { key: "vendor", label: "Brand" },
      { key: "tag", label: "Tag" },
    ],
  });

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
  // Manual ones (design note 0027): the shopper is told how to pay, staff mark it paid.
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
      "Lumen & Loom d.o.o., Demo Bank d.d.",
      "IBAN: HR00 0000 0000 0000 0000 0",
      "BIC: DEMOHR2X",
      "Use your order number as the reference. We ship once the payment arrives.",
    ].join("\n"),
  });

  console.log("Customers and orders");
  const SHOPPERS = [
    {
      email: "ana.horvat@example.com",
      first: "Ana",
      last: "Horvat",
      country: "HR",
      line1: "Vlaška 12",
      city: "Zagreb",
      postal: "10000",
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
  const orders = [];
  for (const shopper of SHOPPERS) {
    const sf = (method, path, body) => call(method, path, body, { key: key.token });
    const cart = await sf("POST", "/storefront/carts", { country: shopper.country });
    const lines = 1 + Math.floor(random() * 3);
    for (let i = 0; i < lines; i++) {
      const product = pick(sellable);
      const variant = pick(product.variants);
      try {
        await sf("POST", `/storefront/carts/${cart.id}/lines`, {
          variant: variant.id,
          quantity: 1 + Math.floor(random() * 2),
        });
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
    orders.push({ order, shopper });
    const paying = order.payment?.method_name ? `, ${order.payment.method_name}` : "";
    console.log(
      `  #${order.number} ${shopper.first} ${shopper.last} (${shopper.country}) ${(priced.total.amount / 100).toFixed(2)} ${priced.total.currency}${paying}`,
    );
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
      reference: cash ? `Courier receipt PE-${4100 + i}` : `HR-TRF-${order.number}-${20261000 + i}`,
    });
    if (i % 2 === 0) {
      const location = shopper.country === "US" ? newJersey.id : zagreb.id;
      await admin.post(`/admin/orders/${order.id}/fulfillments`, {
        location,
        lines: order.lines.map((l) => ({ line: l.id, quantity: l.quantity })),
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

  console.log("\nDone: Lumen & Loom is set up.");
  console.log(
    `  ${sellable.length} products for sale, 1 draft · ${MARKETS.length} markets · 2 payment methods · ${orders.length} orders`,
  );
  console.log("\nFor the storefront (apps/storefront/.dev.vars):");
  console.log(`  STOREFRONT_KEY=${key.token}`);
  if (webhookSecret) console.log(`  HYPHA_WEBHOOK_SECRET=${webhookSecret}`);
}

main().catch((e) => {
  console.error(`\nSeeding failed: ${e.message}`);
  process.exit(1);
});
