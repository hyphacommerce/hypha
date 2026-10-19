#!/usr/bin/env node
// Hypha's CLI (design note 0025): hosting it. `hypha init` writes a deployment (a
// folder of settings and secrets), `hypha deploy` runs it on a server over SSH (or on
// this machine, or the API on Cloudflare Workers), `hypha status` and `hypha logs` look at it,
// and `hypha apps` lists the apps deployed alongside (design note 0029, §9). In this
// repository: `pnpm hypha <command>`.

import { defineCommand, runMain } from "citty";

import { apps } from "./commands/apps.ts";
import { deploy } from "./commands/deploy.ts";
import { init } from "./commands/init.ts";
import { logs, status } from "./commands/ops.ts";

const main = defineCommand({
  meta: {
    name: "hypha",
    version: "0.1.0",
    description: "Host Hypha: the API, the admin, the storefront and the apps beside them",
  },
  subCommands: { init, deploy, status, logs, apps },
});

await runMain(main);
