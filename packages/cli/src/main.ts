#!/usr/bin/env node
// Hypha's CLI: hosting it. `hypha init` writes a deployment (a folder of settings and
// secrets), `hypha deploy` runs it on a server over SSH (or on this machine, or the API on
// Cloudflare Workers), `hypha status` and `hypha logs` look at it, `hypha apps` lists the apps
// deployed alongside and `hypha spores` the spores compiled into the API. In this repository:
// `pnpm hypha <command>`.

import { defineCommand, runMain } from "citty";

import { apps } from "./commands/apps.ts";
import { deploy } from "./commands/deploy.ts";
import { init } from "./commands/init.ts";
import { logs, status } from "./commands/ops.ts";
import { spores } from "./commands/spores.ts";

const main = defineCommand({
  meta: {
    name: "hypha",
    version: "0.1.0-beta.1", // x-release-please-version
    description: "Host Hypha: the API, the admin, the storefront and the apps beside them",
  },
  subCommands: { init, deploy, status, logs, apps, spores },
});

await runMain(main);
