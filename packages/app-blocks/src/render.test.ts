// Renders the fixtures to static markup. The renderer is TSX, which Node doesn't run by
// itself: jiti compiles it on load (as packages/emails does).

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";

import { createJiti } from "jiti";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";

import type { BlockTree } from "./types.ts";

const jiti = createJiti(import.meta.url, { jsx: { runtime: "automatic" } });
const { HyphaBlock } = await jiti.import<typeof import("./render.tsx")>("./render.tsx");

const fixtures = JSON.parse(
  await readFile(new URL("../fixtures/valid.json", import.meta.url), "utf8"),
) as { name: string; tree: BlockTree }[];

function render(tree: unknown): string {
  const original = console.warn;
  console.warn = () => {};
  try {
    return renderToStaticMarkup(
      createElement(HyphaBlock, { tree: tree as BlockTree, onAction: () => {} }),
    );
  } finally {
    console.warn = original;
  }
}

const fixture = (name: string) => {
  const found = fixtures.find((f) => f.name.startsWith(name));
  assert.ok(found, name);
  return found.tree;
};

test("renders the note's example with Astryx components", () => {
  const html = render(fixture("design note"));
  assert.match(html, /class="astryx-card/);
  assert.match(html, /Stripe/);
  assert.match(html, /class="astryx-badge[^"]*" data-variant="success"/);
  assert.match(html, /href="https:\/\/dashboard\.stripe\.com\/payments\/pi_1"/);
  assert.match(html, /target="_blank"/);
  assert.match(html, /Refund/);
});

test("renders every element", () => {
  const html = render(fixture("every element"));
  for (const text of [
    "Reviews",
    "4.6 out of 5",
    "3 reviews wait for moderation",
    "Ratings over time",
    "Great shoes",
    "Too small",
    "Ana",
    "Average",
    "Open dashboard",
    "Delete all",
    "Widget title",
    "Per page",
    "Order",
    "Show photos",
    "Save settings",
  ])
    assert.ok(html.includes(text), `missing ${text}`);
  assert.match(html, /<h2/);
  assert.match(
    html,
    /<img[^>]*src="https:\/\/cdn\.app\.example\/chart\.png"[^>]*alt="Ratings over time"|<img[^>]*alt="Ratings over time"[^>]*src="https:\/\/cdn\.app\.example\/chart\.png"/,
  );
  assert.match(html, /<form/);
  assert.match(html, /type="submit"/);
  assert.match(html, /name="title"[^>]*value="Reviews"|value="Reviews"[^>]*name="title"/);
  // A missing table cell shows a dash.
  assert.ok(html.includes("—"));
});

test("unknown elements render nothing, their siblings render", () => {
  const html = render(fixture("an element a newer version"));
  assert.ok(html.includes("Before"));
  assert.ok(html.includes("After"));
  assert.ok(!html.includes("chart"));
  assert.ok(!html.includes("series"));
});

test("an unknown element warns in development", () => {
  const warnings: unknown[] = [];
  const original = console.warn;
  console.warn = (...args: unknown[]) => warnings.push(args.join(" "));
  try {
    renderToStaticMarkup(
      createElement(HyphaBlock, { tree: fixture("an element a newer version") }),
    );
  } finally {
    console.warn = original;
  }
  assert.deepEqual(warnings, ['[@hyphacommerce/app-blocks] unknown element type "chart": skipped']);
});

test("another version renders nothing", () => {
  assert.equal(render({ version: 2, root: { type: "text", value: "x" } }), "");
});

test("a link or image that isn't https renders nothing even if unvalidated", () => {
  assert.equal(
    render({ version: 1, root: { type: "link", label: "x", href: "javascript:alert(1)" } }),
    "",
  );
  assert.equal(render({ version: 1, root: { type: "image", src: "http://x/a.png", alt: "" } }), "");
});
