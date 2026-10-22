import assert from "node:assert/strict";
import { test } from "node:test";

import { safeNext } from "./safe-next.ts";

test("a local path with its query and fragment is kept", () => {
  assert.equal(safeNext("/orders?status=open#top"), "/orders?status=open#top");
});

test("nothing, or something that isn't a path, goes to the start", () => {
  for (const path of [null, undefined, "", "orders", "https://evil.example/"]) {
    assert.equal(safeNext(path), "/");
  }
});

test("another origin goes to the start, however it's spelled", () => {
  for (const path of [
    "//evil.example",
    "/\\evil.example",
    "/\t/evil.example",
    "/\n/evil.example",
    "/\r/evil.example",
  ]) {
    assert.equal(safeNext(path), "/", JSON.stringify(path));
  }
});

test("the login pages go to the start", () => {
  for (const path of ["/login", "/login?next=/x", "/reset-password/abc", "/session-expired"]) {
    assert.equal(safeNext(path), "/", path);
  }
  assert.equal(safeNext("/logins"), "/logins");
});
