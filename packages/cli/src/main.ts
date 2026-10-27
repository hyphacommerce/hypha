#!/usr/bin/env node
// Hypha's CLI: hosting it. `hypha init` writes a deployment (a folder of settings and
// secrets), `hypha deploy` runs it on a server over SSH (or on this machine, or the API on
// Cloudflare Workers), `hypha status` and `hypha logs` look at it, `hypha apps` lists the apps
// deployed alongside and `hypha spores` the spores compiled into the API; `hypha create`
// starts a project of your own from the storefront or an app. In this repository: `pnpm
// hypha <command>`.

import { defineCommand, runMain } from "citty";

import { apps } from "./commands/apps.ts";
import { createCommand } from "./commands/create.ts";
import { deploy } from "./commands/deploy.ts";
import { init } from "./commands/init.ts";
import { logs, status } from "./commands/ops.ts";
import { spores } from "./commands/spores.ts";
import { VERSION } from "./version.ts";

const main = defineCommand({
  meta: {
    name: "hypha",
    version: VERSION,
    description:
      "Host Hypha: the API, the admin, the storefront and the apps beside them; start your own",
  },
  subCommands: { init, deploy, status, logs, apps, spores, create: createCommand },
});

await runMain(main);
