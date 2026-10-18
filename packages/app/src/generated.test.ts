// The extension points' generated types follow openapi/extensions.json: once the API
// writes that document, src/generated/extensions.ts must be regenerated from it.

import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { access } from "node:fs/promises";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";

const document = new URL("../../../openapi/extensions.json", import.meta.url);
const script = fileURLToPath(new URL("../scripts/generate.ts", import.meta.url));
const present = await access(document).then(
  () => true,
  () => false,
);

test(
  "the generated extension types are current",
  { skip: present ? false : "openapi/extensions.json doesn't exist yet" },
  async () => {
    const run = promisify(execFile);
    await assert.doesNotReject(run(process.execPath, [script, "--check"]));
  },
);
