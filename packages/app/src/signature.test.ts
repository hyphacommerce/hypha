import assert from "node:assert/strict";
import { test } from "node:test";

import { parseSignatureHeader, signRequest, verifySignature } from "./signature.ts";

// The same vector as hypha-http's `a_signature_is_the_hmac_of_time_dot_body`
// (`printf '1700000000.{}' | openssl dgst -sha256 -hmac 'hypha_shs_test'`).
const VECTOR = "t=1700000000,v1=7e9b80e5d75d36d3052ca2dc364618d7dca90cfe78476758f403d6388b8173c7";
const AT = 1_700_000_000;

test("signs like the Rust side", async () => {
  assert.equal(await signRequest("hypha_shs_test", "{}", { timestamp: AT }), VECTOR);
});

test("verifies the Rust side's signature", async () => {
  assert.equal(await verifySignature("hypha_shs_test", VECTOR, "{}", { now: AT }), true);
});

test("verifies bytes and strings alike", async () => {
  const bytes = new TextEncoder().encode("{}");
  assert.equal(await verifySignature("hypha_shs_test", VECTOR, bytes, { now: AT }), true);
  assert.equal(await verifySignature("hypha_shs_test", VECTOR, bytes.buffer, { now: AT }), true);
});

test("rejects another body", async () => {
  assert.equal(await verifySignature("hypha_shs_test", VECTOR, "{ }", { now: AT }), false);
});

test("rejects another secret", async () => {
  assert.equal(await verifySignature("other", VECTOR, "{}", { now: AT }), false);
});

test("accepts a timestamp exactly at the tolerance and rejects one past it", async () => {
  const options = { toleranceSeconds: 60 };
  assert.equal(
    await verifySignature("hypha_shs_test", VECTOR, "{}", { ...options, now: AT + 60 }),
    true,
  );
  assert.equal(
    await verifySignature("hypha_shs_test", VECTOR, "{}", { ...options, now: AT + 61 }),
    false,
  );
  assert.equal(
    await verifySignature("hypha_shs_test", VECTOR, "{}", { ...options, now: AT - 61 }),
    false,
  );
});

test("accepts any of several v1 entries", async () => {
  const header = `t=${AT},v1=${"0".repeat(64)},${VECTOR.split(",")[1]}`;
  assert.equal(await verifySignature("hypha_shs_test", header, "{}", { now: AT }), true);
});

test("rejects missing or malformed headers", async () => {
  for (const header of [
    null,
    undefined,
    "",
    "t=1700000000",
    "v1=abc",
    "t=x,v1=" + "0".repeat(64),
  ]) {
    assert.equal(await verifySignature("hypha_shs_test", header, "{}", { now: AT }), false);
  }
});

test("parses t and v1", () => {
  assert.deepEqual(parseSignatureHeader(VECTOR), {
    timestamp: AT,
    signatures: ["7e9b80e5d75d36d3052ca2dc364618d7dca90cfe78476758f403d6388b8173c7"],
  });
});
