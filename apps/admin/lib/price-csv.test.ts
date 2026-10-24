import assert from "node:assert/strict";
import { test } from "node:test";

import {
  chunk,
  detectDelimiter,
  diffRequests,
  exportPriceCsv,
  importVariantIds,
  parseCsv,
  previewPriceImport,
  toCsv,
} from "./price-csv.ts";
import type { PriceGridProduct, PriceGridVariant } from "./price-grid/types.ts";

const eur = (amount: number) => ({ amount, currency: "EUR" });
const currencies = { EUR: 2, JPY: 0 };

function variant(
  id: string,
  title: string | null,
  sku: string | null,
  fixed: PriceGridVariant["fixed"] = null,
): PriceGridVariant {
  return {
    variant: id,
    title,
    sku,
    base_price: eur(2000),
    compare_at_price: null,
    adjusted_price: null,
    fixed,
  };
}

const products: PriceGridProduct[] = [
  {
    id: "prod_1",
    title: "Grow kit",
    handle: "grow-kit",
    variants: [
      variant("var_s", "Small", "GK-S", {
        price: eur(1800),
        compare_at: eur(2200),
        breaks: [
          { min_quantity: 5, price: eur(1700) },
          { min_quantity: 10, price: eur(1600) },
        ],
      }),
      variant("var_l", "Large, blue", "GK-L"),
    ],
  },
  {
    id: "prod_2",
    title: "Scalpel",
    handle: "scalpel",
    variants: [variant("var_x", null, "DUP"), variant("var_y", "Other", "DUP")],
  },
];

const HEADER = "handle,sku,variant_id,options,base_price,fixed_price,compare_at";

test("export: one row per variant, breaks as wide as the most any has, RFC 4180", () => {
  const csv = exportPriceCsv(products, currencies);
  assert.equal(
    csv,
    [
      `${HEADER},break_1_qty,break_1_price,break_2_qty,break_2_price`,
      "grow-kit,GK-S,var_s,Small,20.00,18.00,22.00,5,17.00,10,16.00",
      'grow-kit,GK-L,var_l,"Large, blue",20.00,,,,,,',
      "scalpel,DUP,var_x,,20.00,,,,,,",
      "scalpel,DUP,var_y,Other,20.00,,,,,,",
      "",
    ].join("\r\n"),
  );
});

test("export: no break columns when no variant has breaks", () => {
  const [header] = parseCsv(exportPriceCsv([products[1]], currencies));
  assert.deepEqual(header, HEADER.split(","));
});

test("export: text a spreadsheet would run as a formula is quoted, and read back", () => {
  const sneaky: PriceGridProduct[] = [
    { ...products[1], variants: [variant("var_x", "=HYPERLINK(1)", "@sku")] },
  ];
  const csv = exportPriceCsv(sneaky, currencies);
  assert.ok(csv.includes(",'@sku,var_x,'=HYPERLINK(1),"));
  const preview = previewPriceImport(
    csv.replace("var_x", ""),
    [{ ...products[1], variants: [variant("var_x", "=HYPERLINK(1)", "@sku")] }],
    "EUR",
    currencies,
  );
  assert.equal(preview.rows[0]?.variant, "var_x");
});

test("an export imports back unchanged", () => {
  const preview = previewPriceImport(
    exportPriceCsv(products, currencies),
    products,
    "EUR",
    currencies,
  );
  assert.deepEqual(preview.counts, { changed: 0, unchanged: 4, unmatched: 0, invalid: 0 });
  assert.deepEqual(preview.diff, { upserts: [], clears: [] });
});

test("parsing: BOM, CRLF or LF, quotes, blank lines", () => {
  assert.deepEqual(parseCsv('﻿a,b\r\n"x, ""y""","line\nbreak"\n\nc,\n'), [
    ["a", "b"],
    ['x, "y"', "line\nbreak"],
    ["c", ""],
  ]);
});

test("parsing: the delimiter is what the header uses most", () => {
  assert.equal(detectDelimiter("a;b;c\n1,5;2;3"), ";");
  assert.equal(detectDelimiter('"a;b",c,d'), ",");
  assert.equal(detectDelimiter("a\tb\tc"), "\t");
  assert.deepEqual(parseCsv("a;b\n12,50;x"), [
    ["a", "b"],
    ["12,50", "x"],
  ]);
});

test("toCsv quotes fields with the delimiter, quotes or line breaks", () => {
  assert.equal(toCsv([["a", "b,c", 'd"e', "f\ng"]]), 'a,"b,c","d""e","f\ng"\r\n');
});

test("rows match by variant ID, then SKU, then handle and options", () => {
  const csv = [
    "variant_id,sku,handle,options,fixed_price",
    "var_l,,,,10",
    ",GK-L,,,11",
    ',,grow-kit,"large, BLUE",12',
  ].join("\n");
  // Each later row finds the same variant its own way, so it's refused as a repeat.
  const preview = previewPriceImport(csv, products, "EUR", currencies);
  assert.deepEqual(
    preview.rows.map((r) => [r.status, r.variant]),
    [
      ["changed", "var_l"],
      ["invalid", "var_l"],
      ["invalid", "var_l"],
    ],
  );
  assert.deepEqual(preview.rows[1]?.reasons, ["The same variant as line 2."]);
});

test("a handle without options finds a product without options", () => {
  const single: PriceGridProduct[] = [{ ...products[1], variants: [variant("var_x", null, null)] }];
  const preview = previewPriceImport("handle,fixed_price\nscalpel,5", single, "EUR", currencies);
  assert.deepEqual(preview.diff.upserts, [
    { variant: "var_x", price: eur(500), compare_at: null, breaks: [] },
  ]);
});

test("unmatched rows say why", () => {
  const csv = [
    "variant_id,sku,handle,options,fixed_price",
    "var_nope,,,,1",
    ",NOPE,,,1",
    ",,nope,,1",
    ",,grow-kit,Huge,1",
    ",,grow-kit,,1",
  ].join("\n");
  const preview = previewPriceImport(csv, products, "EUR", currencies);
  assert.deepEqual(
    preview.rows.map((r) => [r.status, r.reasons[0]]),
    [
      ["unmatched", "No variant var_nope in this list."],
      ["unmatched", "No variant with SKU NOPE."],
      ["unmatched", "No product with handle nope."],
      ["unmatched", "grow-kit has no variant Huge."],
      ["unmatched", "grow-kit has options: fill in the options column."],
    ],
  );
  assert.equal(preview.counts.unmatched, 5);
});

test("a SKU on several variants needs the variant ID", () => {
  const preview = previewPriceImport("sku,fixed_price\nDUP,1", products, "EUR", currencies);
  assert.deepEqual(preview.rows[0], {
    line: 2,
    status: "invalid",
    variant: undefined,
    reasons: ["SKU DUP is on 2 variants: add variant_id."],
  });
});

test("invalid rows list every problem by column", () => {
  const csv = [
    "variant_id,fixed_price,compare_at,break_1_qty,break_1_price,break_2_qty,break_2_price",
    "var_l,abc,,,,,",
    "var_x,10,9,,,,",
    "var_y,10,,5,,x,1",
  ].join("\n");
  const preview = previewPriceImport(csv, products, "EUR", currencies);
  assert.deepEqual(
    preview.rows.map((r) => r.reasons),
    [
      ['fixed_price: "abc" isn\'t an amount.'],
      ["compare_at: Must be above the fixed price."],
      ["Break 1 needs both a quantity and a price.", 'break_2_qty: "x" isn\'t a whole number.'],
    ],
  );
  assert.deepEqual(preview.diff, { upserts: [], clears: [] });
});

test("breaks are checked against the row's fixed price", () => {
  const preview = previewPriceImport(
    "variant_id,fixed_price,break_1_qty,break_1_price\nvar_l,10,5,10",
    products,
    "EUR",
    currencies,
  );
  assert.deepEqual(preview.rows[0]?.reasons, [
    "Breaks: The price from 5 must be below the fixed price.",
  ]);
});

test("changes and removals become the grid's diff, decimal commas and all", () => {
  const csv = [
    "variant_id;fixed_price;compare_at;break_1_qty;break_1_price",
    "var_s;;;;",
    "var_l;12,50;15;3;11,00",
    "var_x;;;;",
  ].join("\r\n");
  const preview = previewPriceImport(csv, products, "EUR", currencies);
  assert.deepEqual(preview.counts, { changed: 2, unchanged: 1, unmatched: 0, invalid: 0 });
  assert.deepEqual(preview.diff, {
    upserts: [
      {
        variant: "var_l",
        price: eur(1250),
        compare_at: eur(1500),
        breaks: [{ min_quantity: 3, price: eur(1100) }],
      },
    ],
    clears: [{ variant: "var_s" }],
  });
  assert.deepEqual(preview.rows[0]?.reasons, ["Removes the fixed price."]);
  assert.deepEqual(preview.rows[1]?.reasons, ["Sets a fixed price."]);
});

test("columns the file doesn't have keep what the variant has", () => {
  const preview = previewPriceImport("sku,fixed_price\nGK-S,19", products, "EUR", currencies);
  assert.deepEqual(preview.diff.upserts, [
    {
      variant: "var_s",
      price: eur(1900),
      compare_at: eur(2200),
      breaks: [
        { min_quantity: 5, price: eur(1700) },
        { min_quantity: 10, price: eur(1600) },
      ],
    },
  ]);
  const removed = previewPriceImport("sku,fixed_price\nGK-S,", products, "EUR", currencies);
  assert.deepEqual(removed.diff.clears, [{ variant: "var_s" }]);
});

test("break columns may skip numbers", () => {
  const preview = previewPriceImport(
    "variant_id,fixed_price,break_1_qty,break_1_price,break_3_qty,break_3_price\nvar_l,10,5,9,10,8",
    products,
    "EUR",
    currencies,
  );
  assert.deepEqual(preview.diff.upserts[0]?.breaks, [
    { min_quantity: 5, price: eur(900) },
    { min_quantity: 10, price: eur(800) },
  ]);
});

test("a file naming every variant by ID needs only those read", () => {
  assert.deepEqual(importVariantIds("Variant_ID,fixed_price\nvar_a,1\nvar_b,2\nvar_a,3"), [
    "var_a",
    "var_b",
  ]);
  assert.equal(importVariantIds("variant_id,sku,fixed_price\nvar_a,,1\n,GK-S,2"), null);
  assert.equal(importVariantIds("sku,fixed_price\nGK-S,2"), null);
  assert.deepEqual(importVariantIds(""), []);
});

test("a step of an import reads only its lines' variants", () => {
  const text = "variant_id,fixed_price\nvar_a,1\nvar_b,2\nvar_c,3";
  assert.deepEqual(importVariantIds(text, { from: 3, to: 4 }), ["var_b", "var_c"]);
  assert.deepEqual(importVariantIds(text, { from: 9, to: 9 }), []);
  // A row without an ID outside the lines still means the whole catalog.
  assert.equal(
    importVariantIds("variant_id,sku,fixed_price\nvar_a,,1\n,GK-S,2", { from: 2, to: 2 }),
    null,
  );
});

test("a file it can't read says why", () => {
  assert.equal(previewPriceImport("", products, "EUR", currencies).error, "The file is empty.");
  assert.equal(
    previewPriceImport("variant_id\nvar_s", products, "EUR", currencies).error,
    "The file has no fixed_price column.",
  );
  assert.equal(
    previewPriceImport("options,fixed_price\nx,1", products, "EUR", currencies).error,
    "The file needs a variant_id, sku or handle column to find each variant.",
  );
  assert.equal(
    previewPriceImport("sku,fixed_price", products, "XYZ", currencies).error,
    'Unknown currency "XYZ".',
  );
});

test("column names are matched ignoring case and spaces", () => {
  const preview = previewPriceImport(
    " Variant_ID ,FIXED_PRICE\nvar_l,9",
    products,
    "EUR",
    currencies,
  );
  assert.equal(preview.counts.changed, 1);
});

test("requests carry at most 1,000 entries each", () => {
  assert.deepEqual(chunk([1, 2, 3, 4, 5], 2), [[1, 2], [3, 4], [5]]);
  assert.deepEqual(chunk([]), []);
  const upserts = Array.from({ length: 2001 }, (_, i) => ({
    variant: `var_${i}`,
    price: eur(100),
    compare_at: null,
    breaks: [],
  }));
  const requests = diffRequests({ upserts, clears: [{ variant: "var_z" }] });
  assert.deepEqual(
    requests.upserts.map((r) => r.length),
    [1000, 1000, 1],
  );
  assert.deepEqual(requests.clears, [["var_z"]]);
});
