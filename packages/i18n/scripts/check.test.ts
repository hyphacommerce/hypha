// Runs the check as a command over the fixture directories.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import pkg from "../package.json" with { type: "json" };

const script = fileURLToPath(new URL("./check.ts", import.meta.url));
const fixture = (name: string) => fileURLToPath(new URL(`../fixtures/${name}`, import.meta.url));

function check(...args: string[]) {
  const run = spawnSync(process.execPath, [script, ...args], { encoding: "utf8" });
  return { status: run.status, out: run.stdout, err: run.stderr };
}

test("passes, printing what each language still lacks", () => {
  const { status, out, err } = check(fixture("ok"));
  assert.equal(status, 0, err);
  assert.equal(
    out,
    [
      "de: 2 messages, 3 missing",
      "  cart.terms",
      "  checkout.greeting",
      "  checkout.pay",
      "en: 5 messages (the source)",
      "sr-Latn: 5 messages, complete",
      "",
    ].join("\n"),
  );
  assert.equal(err, "");
});

test("--complete fails a language that lacks a key, and passes one that doesn't", () => {
  const behind = check(fixture("ok"), "--complete", "de");
  assert.equal(behind.status, 1);
  assert.match(behind.err, /error: de: cart\.terms: must be translated \(missing_key\)/);
  assert.match(behind.err, /3 problems/);

  const done = check(fixture("ok"), "--complete", "sr-Latn");
  assert.equal(done.status, 0, done.err);

  const both = check(fixture("ok"), "--complete", "sr-Latn", "--complete", "de");
  assert.equal(both.status, 1);
});

test("--complete with a language that has no catalog fails", () => {
  const { status, err } = check(fixture("ok"), "--complete", "fr");
  assert.equal(status, 1);
  assert.match(err, /error: fr: has no catalog \(missing_language\)/);
});

test("a key English lacks fails", () => {
  const { status, err } = check(fixture("extra-key"));
  assert.equal(status, 1);
  assert.equal(err, "error: de: cart.subtitle: en has no such key (unknown_key)\n1 problem\n");
});

test("placeholders or tags other than English's fail", () => {
  const { status, err } = check(fixture("placeholders"));
  assert.equal(status, 1);
  assert.equal(
    err,
    "error: de: cart.greeting: lacks placeholder name; has placeholder vorname the source doesn't (placeholder_mismatch)\n" +
      "error: de: cart.terms: lacks tag link; has tag b the source doesn't (placeholder_mismatch)\n" +
      "2 problems\n",
  );
});

test("a message that doesn't parse, or isn't a string, fails", () => {
  const { status, err } = check(fixture("syntax"));
  assert.equal(status, 1);
  assert.match(err, /error: en: cart\.items_count: .+ \(invalid_message\)/);
  assert.match(err, /error: de: cart\.title: isn't a string \(invalid_message\)/);
});

test("a file per language with full keys works too", () => {
  const { status, out } = check(fixture("flat"));
  assert.equal(status, 0);
  assert.equal(out, "de: 1 message, 1 missing\n  cart.empty\nen: 2 messages (the source)\n");
});

test("a directory with both layouts is refused", () => {
  const { status, err } = check(fixture("mixed"));
  assert.equal(status, 2);
  assert.match(err, /mixes two layouts: en\/ \(a folder per language\) and de\.json/);
});

test("an empty translation is listed as missing, and fails --complete", () => {
  const listed = check(fixture("empty"));
  assert.equal(listed.status, 0, listed.err);
  assert.equal(
    listed.out,
    "de: 2 messages, 1 missing\n  cart.empty\nen: 2 messages (the source)\n",
  );
  const strict = check(fixture("empty"), "--complete", "de");
  assert.equal(strict.status, 1);
  assert.match(strict.err, /error: de: cart\.empty: must be translated \(missing_key\)/);
});

test("another source language", () => {
  const { status, err } = check(fixture("flat"), "--source", "de");
  assert.equal(status, 1);
  assert.match(err, /error: en: cart\.empty: de has no such key \(unknown_key\)/);
});

test("exits 2 when it can't run", () => {
  assert.equal(check().status, 2);
  assert.equal(check(fixture("nowhere")).status, 2);
  assert.equal(check(fixture("ok"), "--nope").status, 2);
});

test("--version prints the package's", () => {
  const { status, out } = check("--version");
  assert.equal(status, 0);
  assert.equal(out.trim(), pkg.version);
});
