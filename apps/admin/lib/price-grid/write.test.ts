import assert from "node:assert/strict";
import { test } from "node:test";

import type { FixedPriceUpsert } from "./types.ts";
import { type DiffWriter, writeDiff } from "./write.ts";

const entry = (variant: string): FixedPriceUpsert => ({
  variant,
  price: { amount: 1000, currency: "EUR" },
  compare_at: null,
  breaks: [],
});

function recording(answers: DiffWriter["put"]) {
  const puts: string[][] = [];
  const writer: DiffWriter = {
    put: (entries) => {
      puts.push(entries.map((e) => e.variant));
      return answers(entries);
    },
    remove: async () => ({}),
    describe: (error) => error.error,
  };
  return { puts, writer };
}

test("a PUT refused for one entry is sent again without it", async () => {
  const { puts, writer } = recording(async (entries) =>
    entries.some((e) => e.variant === "var_1")
      ? {
          error: {
            code: "fixed_prices_invalid",
            error: "Some fixed prices are invalid.",
            details: {
              entries: [{ index: 1, code: "price_too_large", message: "Too large." }],
            },
          },
        }
      : {},
  );
  const result = await writeDiff(
    { upserts: [entry("var_0"), entry("var_1"), entry("var_2")], clears: [] },
    writer,
  );
  assert.deepEqual(puts, [
    ["var_0", "var_1", "var_2"],
    ["var_0", "var_2"],
  ]);
  assert.deepEqual(Object.keys(result.rowErrors ?? {}), ["var_1"]);
  assert.equal(result.error, undefined);
});

test("a PUT refused as a whole marks every entry", async () => {
  const { writer } = recording(async () => ({
    error: { code: "price_list_not_found", error: "No such list." },
  }));
  const result = await writeDiff({ upserts: [entry("var_0"), entry("var_1")], clears: [] }, writer);
  assert.deepEqual(result, {
    error: "No such list.",
    rowErrors: { var_0: "No such list.", var_1: "No such list." },
  });
});
