import assert from "node:assert/strict";
import { test } from "node:test";

import {
  CODE_ALPHABET,
  codesFrom,
  generateCode,
  generationProblem,
  normaliseCode,
} from "./codes.ts";

test("typed codes are split on lines, commas and spaces, each kept once as compared", () => {
  assert.deepEqual(codesFrom("WELCOME10\nspring, Welcome10;\n\n  AUTUMN  "), [
    "WELCOME10",
    "spring",
    "AUTUMN",
  ]);
  assert.deepEqual(codesFrom(" \n,"), []);
});

test("codes are compared trimmed, uppercase and NFKC", () => {
  assert.equal(normaliseCode("  welcome10 "), "WELCOME10");
  // A full-width "Ａ" is an "A".
  assert.equal(normaliseCode("Ａb"), "AB");
});

test("codes are uppercased then normalised again, as the API stores them", () => {
  // "ΐ" (U+0390) uppercases to Ι + diaeresis + acute, which NFKC composes to Ϊ + acute.
  assert.equal(normaliseCode("ΐ"), "Ϊ́");
});

test("generated codes use only the alphabet, the prefix counted in the length", () => {
  const code = generateCode(12, "vip-");
  assert.equal(code.length, 12);
  assert.ok(code.startsWith("VIP-"));
  for (const c of code.slice(4)) assert.ok(CODE_ALPHABET.includes(c), c);
  assert.equal(generateCode().length, 12);
});

test("a generated code keeps at least 8 random characters after its prefix", () => {
  assert.equal(generationProblem(8, ""), null);
  assert.equal(generationProblem(12, "VIP-"), null);
  assert.equal(
    generationProblem(11, "VIP-"),
    "Codes need 8 random characters after the 4-character prefix: a length of 12 or more.",
  );
  assert.equal(
    generationProblem(24, "ABCDEFGHIJKLMNOPQ"),
    "The prefix is 17 characters: at most 16, so codes keep 8 random characters.",
  );
  assert.throws(() => generateCode(11, "VIP-"), RangeError);
});

test("generated lengths are 8 to 24", () => {
  const message = "Length: 8 to 24 characters, the prefix included.";
  assert.equal(generationProblem(7, ""), message);
  assert.equal(generationProblem(25, ""), message);
  assert.equal(generationProblem(24, ""), null);
});

test("the alphabet has no look-alikes", () => {
  for (const c of "0O1IL") assert.equal(CODE_ALPHABET.includes(c), false, c);
});

test("bytes that would bias the draw are drawn again", () => {
  // 31 characters: 248 and up are refused, so the first draw yields only five.
  const draws = [new Uint8Array([255, 250, 0, 248, 1, 2, 3, 4]), new Uint8Array([5, 6, 7, 8])];
  let call = 0;
  const random = (bytes: Uint8Array) => {
    const next = draws[call++] ?? bytes;
    bytes.set(next.slice(0, bytes.length));
    return bytes;
  };
  assert.equal(generateCode(8, "", random), "ABCDEFGH");
});
