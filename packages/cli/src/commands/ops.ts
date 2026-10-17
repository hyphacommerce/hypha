// Looking at a running deployment: `hypha status` and `hypha logs`.

import * as p from "@clack/prompts";
import { defineCommand } from "citty";

import { healthy } from "../api.ts";
import { url } from "../deployment.ts";
import { quote } from "../shell.ts";
import { dirArg, guarded, open } from "./shared.ts";

export const status = defineCommand({
  meta: { name: "status", description: "What's running, and whether the API answers" },
  args: { dir: dirArg },
  run: ({ args }) =>
    guarded(async () => {
      const { d, machine } = await open(args.dir);
      p.intro(`hypha status → ${machine.label}`);
      await machine.sh("docker compose ps --format 'table {{.Service}}\\t{{.Status}}'");
      const ok = await healthy(d);
      p.outro(`${url(d, d.domains.api)}/health ${ok ? "answers" : "doesn't answer"}`);
      if (!ok) process.exitCode = 1;
    }),
});

export const logs = defineCommand({
  meta: { name: "logs", description: "A service's logs (api, admin, storefront, caddy, postgres, s3), or all" },
  args: {
    dir: dirArg,
    service: { type: "positional", required: false, description: "One service; all without" },
    follow: { type: "boolean", alias: "f", description: "Keep printing new lines" },
    tail: { type: "string", default: "200", description: "Lines to start with" },
  },
  run: ({ args }) =>
    guarded(async () => {
      const { machine } = await open(args.dir);
      const service = args.service ? ` ${quote(args.service)}` : "";
      await machine.sh(`docker compose logs --tail ${quote(args.tail)}${args.follow ? " -f" : ""}${service}`);
    }),
});
