#!/usr/bin/env node
// Hypha's CLI (design note 0025): hosting it. `hypha init` writes a deployment (a
// folder of settings and secrets), `hypha deploy` runs it on a server over SSH (or on
// this machine), `hypha status` and `hypha logs` look at it. In this repository:
// `pnpm hypha <command>`.

import { defineCommand, runMain } from "citty";

import { deploy } from "./commands/deploy.ts";
import { init } from "./commands/init.ts";
import { logs, status } from "./commands/ops.ts";

const main = defineCommand({
  meta: { name: "hypha", version: "0.1.0", description: "Host Hypha: the API, the admin and the storefront" },
  subCommands: { init, deploy, status, logs },
});

await runMain(main);
