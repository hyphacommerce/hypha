// Looking at a running deployment: `hypha status` and `hypha logs`.

import * as p from "@clack/prompts";
import { defineCommand } from "citty";

import { healthy, waitForAnswer } from "../api.ts";
import { appUrl, appWorkerName } from "../apps.ts";
import { appWorker, workerName } from "../cloudflare.ts";
import { apps, type Deployment, loadDeployment, loadEnv, url } from "../deployment.ts";
import { quote, run } from "../shell.ts";
import { dirArg, guarded, open } from "./shared.ts";

export const status = defineCommand({
  meta: { name: "status", description: "What's running, and whether the API answers" },
  args: { dir: dirArg },
  run: ({ args }) =>
    guarded(async () => {
      const deployment = await loadDeployment(args.dir);
      if (deployment.target === "cloudflare") {
        p.intro("hypha status → Cloudflare");
        let ok = await healthy(deployment);
        p.log.step(
          `${workerName(deployment)}  ${url(deployment, deployment.domains.api)}/health ${ok ? "answers" : "doesn't answer"}`,
        );
        for (const app of apps(deployment)) {
          const target = url(deployment, deployment.domains[app]!);
          const answers = await waitForAnswer(target, 1);
          ok &&= answers;
          p.log.step(
            `${appWorker(deployment, app)}  ${target} ${answers ? "answers" : "doesn't answer"}`,
          );
        }
        for (const entry of deployment.apps ?? []) {
          const answers = await waitForAnswer(`${appUrl(deployment, entry)}/health`, 1);
          ok &&= answers;
          p.log.step(
            `${appWorkerName(deployment, entry)}  ${appUrl(deployment, entry)} ${answers ? "answers" : "doesn't answer"}`,
          );
        }
        p.outro(
          ok
            ? "Everything answers (whether the apps are installed: `hypha apps status`)"
            : "Something doesn't answer: see `hypha logs <api|admin|storefront|app>`",
        );
        if (!ok) process.exitCode = 1;
        return;
      }
      const { d, machine } = await open(args.dir);
      p.intro(`hypha status → ${machine.label}`);
      await machine.sh("docker compose ps --format 'table {{.Service}}\\t{{.Status}}'");
      const ok = await healthy(d);
      p.outro(`${url(d, d.domains.api)}/health ${ok ? "answers" : "doesn't answer"}`);
      if (!ok) process.exitCode = 1;
    }),
});

export const logs = defineCommand({
  meta: {
    name: "logs",
    description:
      "A service's logs (api, admin, storefront, caddy, postgres, s3, app-<name>), or all",
  },
  args: {
    dir: dirArg,
    service: { type: "positional", required: false, description: "One service; all without" },
    follow: { type: "boolean", alias: "f", description: "Keep printing new lines" },
    tail: { type: "string", default: "200", description: "Lines to start with" },
  },
  run: ({ args }) =>
    guarded(async () => {
      const deployment = await loadDeployment(args.dir);
      if (deployment.target === "cloudflare") {
        // A Worker's live logs (Workers Logs keeps the past ones, in the dashboard).
        const worker = cloudflareWorker(deployment, args.service);
        const env: Record<string, string> = {};
        const token = (await loadEnv(args.dir).catch(() => undefined))?.get("CLOUDFLARE_API_TOKEN");
        if (token) env.CLOUDFLARE_API_TOKEN = token;
        if (deployment.cloudflare?.accountId)
          env.CLOUDFLARE_ACCOUNT_ID = deployment.cloudflare.accountId;
        await run("wrangler", ["tail", worker, "--format", "pretty"], { env });
        return;
      }
      const { machine } = await open(args.dir);
      const service = args.service ? ` ${quote(args.service)}` : "";
      await machine.sh(
        `docker compose logs --tail ${quote(args.tail)}${args.follow ? " -f" : ""}${service}`,
      );
    }),
});

/** The Worker `service` names (api by default) in a Cloudflare deployment. */
function cloudflareWorker(d: Deployment, service: string | undefined): string {
  if (!service || service === "api") return workerName(d);
  if (service === "admin" || service === "storefront") {
    if (!d.domains[service]) throw new Error(`this deployment has no ${service}`);
    return appWorker(d, service);
  }
  // A co-located app, by its name or its server's service name (`app-stripe`).
  const app = (d.apps ?? []).find((a) => service === a.name || service === `app-${a.name}`);
  if (app) return appWorkerName(d, app);
  throw new Error(
    `on Cloudflare, logs are per Worker: api, admin, storefront${(d.apps ?? []).map((a) => `, ${a.name}`).join("")} (not ${service})`,
  );
}
