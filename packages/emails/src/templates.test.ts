// Guards the contract between the built defaults and the API: every file exists, blocks
// are balanced, the layout has exactly its two blocks, and each template names only
// variables its context has. A default that names one its context lacks would fail on
// every send (the API renders with strict undefined).

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";

const dir = new URL("../../../crates/hypha-notifications/defaults/", import.meta.url);

const keys = [
  "order_confirmation",
  "order_shipped",
  "order_tracking_updated",
  "order_cancelled",
  "staff_new_order",
  "staff_invite",
  "staff_password_reset",
] as const;
type Key = (typeof keys)[number];

const money = ["amount", "currency", "formatted"];
const time = ["iso", "formatted"];
const nested = (root: string, fields: string[]) => fields.map((field) => `${root}.${field}`);

const every = [
  ...nested("shop", ["name", "email", "url"]),
  ...nested("brand", ["logo_url", "accent", "footer"]),
];
const order = [
  ...nested("order", [
    "id",
    "number",
    "url",
    "email",
    "taxes_included",
    "shipping_method",
    "lines[]",
  ]),
  ...nested("order.placed_at", time),
  ...nested("order.lines[]", ["title", "variant", "sku", "quantity"]),
  ...nested("order.lines[].unit_price", money),
  ...nested("order.lines[].total", money),
  ...["subtotal", "shipping", "tax", "total"].flatMap((name) => nested(`order.${name}`, money)),
  ...nested("order.shipping_address", [
    "name",
    "company",
    "line1",
    "line2",
    "city",
    "province",
    "postal_code",
    "country",
    "phone",
  ]),
  ...nested("order.payment", ["method", "status", "instructions"]),
  "customer.name",
];
const fulfillment = [
  ...nested("fulfillment.lines[]", ["title", "variant", "quantity"]),
  ...nested("fulfillment.tracking[]", ["carrier", "number", "url"]),
];
const staff = ["user.email", "link", ...nested("expires_at", time)];

const contexts: Record<Key | "layout", string[]> = {
  layout: every,
  order_confirmation: [...every, ...order],
  order_shipped: [...every, ...order, ...fulfillment],
  order_tracking_updated: [...every, ...order, ...fulfillment],
  order_cancelled: [...every, ...order, "cancellation.reason"],
  staff_new_order: [...every, ...order, "admin_url"],
  staff_invite: [...every, ...staff],
  staff_password_reset: [...every, ...staff],
};

/** Every path and its prefixes (`order.payment` is tested before its fields are read). */
function allowed(paths: string[]): Set<string> {
  const set = new Set<string>();
  for (const path of paths) {
    const parts = path.split(".");
    for (let end = 1; end <= parts.length; end += 1) {
      const prefix = parts.slice(0, end).join(".");
      set.add(prefix);
      // A list's items are `list[]`; the list itself is tested and looped over.
      set.add(prefix.replace(/\[\]$/, ""));
    }
  }
  return set;
}

const KEYWORDS = new Set(["and", "or", "not", "in", "is", "none", "true", "false"]);

interface Tag {
  kind: "print" | "statement";
  body: string;
}

function tags(source: string): Tag[] {
  return [...source.matchAll(/\{\{-?\s*(.*?)\s*-?\}\}|\{%-?\s*(.*?)\s*-?%\}/gs)].map((match) =>
    match[1] === undefined
      ? { kind: "statement", body: match[2] ?? "" }
      : { kind: "print", body: match[1] },
  );
}

/**
 * Checks one template: balanced `if` / `for` / `block`, no filters (so no `|safe`), and
 * every path in its context, a `for`'s variable standing for its list's items. Returns
 * the block names it defines.
 */
function verify(name: string, source: string, context: Set<string>): string[] {
  const open: string[] = [];
  const loops = new Map<string, string>();
  const blocks: string[] = [];

  const paths = (expression: string) => {
    assert.ok(!expression.includes("|"), `${name}: a filter in ${JSON.stringify(expression)}`);
    const words = expression
      .replace(/"[^"]*"|'[^']*'/g, "")
      .match(/[a-z_][a-z0-9_]*(?:\.[a-z_][a-z0-9_]*)*/g);
    for (const word of words ?? []) {
      if (KEYWORDS.has(word)) continue;
      const [root = "", ...rest] = word.split(".");
      const path = [loops.get(root) ?? root, ...rest].join(".");
      assert.ok(context.has(path), `${name}: ${word} isn't in its context`);
    }
  };

  for (const [index, tag] of tags(source).entries()) {
    if (tag.kind === "print") {
      paths(tag.body);
      continue;
    }
    const [keyword = "", ...rest] = tag.body.split(/\s+/);
    const argument = rest.join(" ");
    switch (keyword) {
      case "extends":
        assert.equal(index, 0, `${name}: extends must come first`);
        assert.equal(argument, '"layout"', `${name}: extends ${argument}`);
        break;
      case "block":
        assert.match(argument, /^(preheader|content)$/, `${name}: block ${argument}`);
        blocks.push(argument);
        open.push("block");
        break;
      case "if":
        paths(argument);
        open.push("if");
        break;
      case "elif":
        paths(argument);
        assert.equal(open.at(-1), "if", `${name}: elif outside an if`);
        break;
      case "else":
        assert.equal(open.at(-1), "if", `${name}: else outside an if`);
        break;
      case "for": {
        const head = /^([a-z_][a-z0-9_]*) in (.+)$/.exec(argument);
        assert.ok(head, `${name}: for ${argument}`);
        const [, variable = "", list = ""] = head;
        paths(list);
        const [root = "", ...fields] = list.split(".");
        loops.set(variable, `${[loops.get(root) ?? root, ...fields].join(".")}[]`);
        open.push(`for ${variable}`);
        break;
      }
      case "endif":
      case "endblock":
      case "endfor": {
        const closed = open.pop();
        assert.ok(closed?.startsWith(keyword.slice(3)), `${name}: ${keyword} closes ${closed}`);
        if (closed?.startsWith("for ")) loops.delete(closed.slice(4));
        break;
      }
      default:
        assert.fail(`${name}: unexpected tag {% ${tag.body} %}`);
    }
  }
  assert.deepEqual(open, [], `${name}: unclosed ${open.join(", ")}`);
  return blocks;
}

const read = (file: string) => readFile(new URL(file, dir), "utf8");

test("the layout defines exactly the preheader and content blocks", async () => {
  const blocks = verify("layout", await read("layout.html.j2"), allowed(contexts.layout));
  assert.deepEqual(blocks, ["preheader", "content"]);
});

for (const key of keys) {
  test(`${key}: each file uses only its context's variables`, async () => {
    const context = allowed(contexts[key]);
    const subject = await read(`${key}.subject.j2`);
    assert.ok(!subject.includes("\n"), `${key}: the subject is one line`);
    verify(`${key}.subject`, subject, context);

    const html = await read(`${key}.html.j2`);
    assert.ok(html.startsWith('{% extends "layout" %}'), `${key}: html extends the layout`);
    assert.deepEqual(verify(`${key}.html`, html, context), ["preheader", "content"]);

    const text = await read(`${key}.txt.j2`);
    assert.ok(text.trim().length > 0, `${key}: a plain-text body`);
    assert.deepEqual(verify(`${key}.txt`, text, context), [], `${key}: text has no blocks`);
  });
}
