// The fixtures hold the validator and the published JSON Schema to the same answers.

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";

import { Ajv2020 } from "ajv/dist/2020.js";

import { MAX_DEPTH, validateBlock } from "./validate.ts";

const read = async (path: string) =>
  JSON.parse(await readFile(new URL(path, import.meta.url), "utf8")) as unknown;

const schema = (await read("../schema/block.v1.json")) as object;
const valid = (await read("../fixtures/valid.json")) as {
  name: string;
  tree: unknown;
  warnings?: string[];
}[];
const invalid = (await read("../fixtures/invalid.json")) as {
  name: string;
  tree: unknown;
  error: string;
}[];

const ajv = new Ajv2020({
  allErrors: true,
  strict: true,
  strictRequired: false,
  allowUnionTypes: true,
});
const schemaAccepts = ajv.compile(schema);

for (const fixture of valid) {
  test(`valid: ${fixture.name}`, () => {
    const result = validateBlock(fixture.tree);
    assert.deepEqual(result.errors, []);
    assert.equal(result.valid, true);
    assert.deepEqual(
      result.warnings.map((w) => w.path),
      fixture.warnings ?? [],
    );
    assert.equal(schemaAccepts(fixture.tree), true, JSON.stringify(schemaAccepts.errors));
  });
}

for (const fixture of invalid) {
  test(`invalid: ${fixture.name}`, () => {
    const result = validateBlock(fixture.tree);
    assert.equal(result.valid, false);
    assert.ok(
      result.errors.some((e) => e.path === fixture.error),
      `expected an error at ${fixture.error}, got ${JSON.stringify(result.errors)}`,
    );
    assert.equal(schemaAccepts(fixture.tree), false, "the schema accepts it");
  });
}

function nested(depth: number): unknown {
  let element: unknown = { type: "divider" };
  for (let i = 1; i < depth; i++) element = { type: "stack", children: [element] };
  return { version: 1, root: element };
}

test(`trees nest up to ${MAX_DEPTH} elements deep`, () => {
  assert.equal(validateBlock(nested(MAX_DEPTH)).valid, true);
  assert.equal(validateBlock(nested(MAX_DEPTH + 1)).valid, false);
});

test("anything that isn't an object is invalid", () => {
  for (const value of [null, "x", 1, []]) assert.equal(validateBlock(value).valid, false);
});
