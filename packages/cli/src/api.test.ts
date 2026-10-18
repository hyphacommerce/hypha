import assert from "node:assert/strict";
import type { LookupFunction } from "node:net";
import { test } from "node:test";

import { lookupVia } from "./api.ts";

type Answer = { error: Error | null; address?: unknown; family?: number };

/** Runs `lookup` once and returns what it called back with. */
function ask(lookup: LookupFunction, host: string, options: { all?: boolean; family?: number } = {}): Promise<Answer> {
  return new Promise((resolve) =>
    lookup(host, options, (error, address, family) => resolve({ error, address, family })),
  );
}

const system: LookupFunction = (host, _options, callback) => callback(null, `system:${host}`, 4);
const known = (addresses: string[]) => async () => addresses;
const unknown = async (): Promise<string[]> => {
  throw new Error("ENOTFOUND");
};

test("a name the public resolvers know is answered by them, IPv4 first, not the local cache", async () => {
  const lookup = lookupVia(known(["104.21.64.70"]), known(["2606:4700:3035::6815:4046"]), system);
  assert.deepEqual(await ask(lookup, "api.example.com"), { error: null, address: "104.21.64.70", family: 4 });
  assert.deepEqual(await ask(lookup, "api.example.com", { all: true }), {
    error: null,
    address: [
      { address: "104.21.64.70", family: 4 },
      { address: "2606:4700:3035::6815:4046", family: 6 },
    ],
    family: undefined,
  });
  assert.deepEqual(await ask(lookup, "api.example.com", { family: 6 }), {
    error: null,
    address: "2606:4700:3035::6815:4046",
    family: 6,
  });
});

test("names they don't know, and IP literals, go to this machine's resolver", async () => {
  const lookup = lookupVia(unknown, unknown, system);
  assert.equal((await ask(lookup, "intranet.local")).address, "system:intranet.local");
  const literal = lookupVia(known(["1.1.1.1"]), known([]), system);
  assert.equal((await ask(literal, "203.0.113.7")).address, "system:203.0.113.7");
});
