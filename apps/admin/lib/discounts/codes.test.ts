import assert from "node:assert/strict";
import { test } from "node:test";

import { CODE_ALPHABET, codesFrom, generateCode, normaliseCode } from "./codes.ts";

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

test("generated codes use only the alphabet, at the length asked, after the prefix", () => {
  const code = generateCode(12, "vip-");
  assert.equal(code.length, 16);
  assert.ok(code.startsWith("VIP-"));
  for (const c of code.slice(4)) assert.ok(CODE_ALPHABET.includes(c), c);
});

test("the alphabet has no look-alikes", () => {
  for (const c of "0O1IL") assert.equal(CODE_ALPHABET.includes(c), false, c);
});

test("bytes that would bias the draw are drawn again", () => {
  // 31 characters: 248 and up are refused, so the first draw yields only one.
  const draws = [new Uint8Array([255, 250, 0]), new Uint8Array([1, 2, 3])];
  let call = 0;
  const random = (bytes: Uint8Array) => {
    const next = draws[call++] ?? bytes;
    bytes.set(next.slice(0, bytes.length));
    return bytes;
  };
  assert.equal(generateCode(3, "", random), "ABC");
});
