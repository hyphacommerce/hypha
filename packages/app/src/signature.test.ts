import assert from "node:assert/strict";
import { test } from "node:test";

import {
  contextSigningInput,
  parseSignatureHeader,
  signRequest,
  verifyContextSignature,
  verifyForwardedRequest,
  verifySignature,
} from "./signature.ts";

// The same vector as hypha-http's `a_context_signature_is_the_shared_vector`.
const CONTEXT_VECTOR =
  "t=1700000000,v1=5030bb394419b2ce94b7ed9a9f91e6165d0dedbc34e6af9c0da11c3c38a87e6f";

test("a context signature signs the method and the Hypha-* headers, sorted", async () => {
  const headers = new Headers({
    "Hypha-Store": "store_1",
    "Hypha-Staff": "user_1",
    "Hypha-Signature": "t=1,v1=00",
    "Content-Type": "application/json",
    "Hypha-Context-Signature": CONTEXT_VECTOR,
  });
  assert.equal(
    contextSigningInput("get", headers),
    "GET\nhypha-staff:user_1\nhypha-store:store_1\n",
  );
  const at = { now: 1_700_000_000 };
  assert.equal(await verifyContextSignature("hypha_as_test", "GET", headers, at), true);
  assert.equal(await verifyContextSignature("hypha_as_test", "POST", headers, at), false);
  headers.set("Hypha-Staff", "user_2");
  assert.equal(await verifyContextSignature("hypha_as_test", "GET", headers, at), false);
});

test("a forwarded request needs both signatures", async () => {
  const at = 1_700_000_000;
  const headers = new Headers({ "Hypha-Store": "store_1", "Hypha-Staff": "user_1" });
  headers.set("Hypha-Context-Signature", CONTEXT_VECTOR);
  headers.set("Hypha-Signature", await signRequest("hypha_as_test", "", { timestamp: at }));
  const request = { method: "GET", headers };
  assert.equal(await verifyForwardedRequest("hypha_as_test", request, "", { now: at }), true);
  assert.equal(await verifyForwardedRequest("hypha_as_test", request, "x", { now: at }), false);
});

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
