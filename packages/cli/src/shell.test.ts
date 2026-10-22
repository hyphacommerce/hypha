import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { test } from "node:test";

import { quote } from "./shell.ts";

test("plain words stay as they are", () => {
  assert.equal(quote("hypha-api"), "hypha-api");
  assert.equal(quote("root@203.0.113.7:/opt/hypha"), "root@203.0.113.7:/opt/hypha");
});

test("anything else reaches the shell as one literal word", () => {
  for (const value of ["two words", "it's", "$(rm -rf ~)", "`id`", "a;b", "", "'\\'\"", "$HOME"]) {
    const echoed = execFileSync("sh", ["-c", `printf %s ${quote(value)}`], { encoding: "utf8" });
    assert.equal(echoed, value, value);
  }
});
