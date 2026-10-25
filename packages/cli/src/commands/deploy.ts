// `hypha deploy`: checks the server, points it at the published images of this CLI's
// version (or builds them from the checkout: `--build-from-source`, `hypha init
// --from-source`, or a deployment with spores), copies the deployment's files, starts
// the stack and waits for it; the first time, also connects the storefront. Run it again
// after any change (a newer CLI, `.env`, `hypha.config.toml`): it's the same steps, and
// Compose restarts only what changed.

import { resolve4, resolve6 } from "node:dns/promises";
import { isIP } from "node:net";
import { readFile, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";

import * as p from "@clack/prompts";
import { defineCommand } from "citty";

import { Staff, waitForAnswer, waitForHealth } from "../api.ts";
import {
  APPS_CADDY_FILE,
  type AppEntry,
  type AppProfile,
  appDomain,
  appUrl,
  CADDY_IMPORT,
  checkout,
  cloneDir,
  COMPOSE_APPS_FILE,
  composeApps,
  readProfile,
  service,
} from "../apps.ts";
import { deployCloudflare, hereTools } from "../cloudflare.ts";
import {
  apps,
  type Deployment,
  ENV_FILE,
  loadDeployment,
  loadEnv,
  saveDeployment,
  saveEnv,
  url,
} from "../deployment.ts";
import { appImage, build, images, publishedImage, setImageRefs } from "../images.ts";
import { CommandFailed, type Machine, run } from "../shell.ts";
import { fetchSpores, writeHostCrate } from "../spores.ts";
import { appsCaddy, requireRepo } from "../templates.ts";
import { VERSION } from "../version.ts";
import { ensureEncryptionKeys, offerInstalls, pinUnpinned } from "./apps.ts";
import { connectStorefront, syncWebhook } from "../storefront.ts";
import { dirArg, guarded, open } from "./shared.ts";
import { obtainToken } from "./token.ts";

/** What the server runs: copied on every deploy. */
const FILES = [
  ENV_FILE,
  "compose.yaml",
  COMPOSE_APPS_FILE,
  "Caddyfile",
  APPS_CADDY_FILE,
  "hypha.config.toml",
  "hypha.config.schema.json",
  "seaweedfs/s3.json",
];

export const deploy = defineCommand({
  meta: {
    name: "deploy",
    description: "Build, copy and start the deployment (again after any change)",
  },
  args: {
    dir: dirArg,
    "build-from-source": {
      type: "boolean",
      description: "Build the images from this checkout instead of pulling the published ones",
    },
    "skip-build": {
      type: "boolean",
      description: "When building from source: use the images already there",
    },
    images: {
      type: "string",
      description: `Pull the published images at this version instead of ${VERSION}`,
    },
    yes: {
      type: "boolean",
      alias: "y",
      description: "Don't ask (installing Docker, DNS warnings)",
    },
  },
  run: ({ args }) =>
    guarded(async () => {
      const dir = args.dir;
      const deployment = await loadDeployment(dir);
      await pinUnpinned(dir, deployment);
      if (deployment.target === "cloudflare") {
        p.intro("hypha deploy → Cloudflare");
        // The Workers are built here: the API with worker-build, the apps with Vite.
        requireRepo("Deploying to Cloudflare");
        const env = await loadEnv(dir);
        const cf = deployment.cloudflare;
        const known = env.get("CLOUDFLARE_API_TOKEN");
        const colocated = Boolean(deployment.apps?.length);
        // A deployment made before the token, or one whose token was removed: ask here
        // rather than send the person through `init` again. With co-located apps, check
        // the token we have too, before anything deploys: an older token lacks D1, and
        // finding out at the first app's database would leave a half-done deploy.
        if (cf && ((!known && apps(deployment).length > 0) || colocated)) {
          const { token, checked } = await obtainToken({
            name: cf.name,
            domain: deployment.domains.api,
            // The token `.env` has is checked as is, never asked for again.
            given: known,
            accountId: cf.accountId,
            zoneId: cf.zoneId,
            quiet: args.yes,
            apps: colocated,
          });
          env.set("CLOUDFLARE_API_TOKEN", token);
          await saveEnv(dir, env);
          if (!cf.accountId || !cf.zoneId) {
            Object.assign(cf, { accountId: checked.accountId, zoneId: checked.zoneId });
            await saveDeployment(dir, deployment);
          }
        }
        await deployCloudflare(
          resolve(dir),
          deployment,
          env,
          hereTools(resolve(dir), deployment, env),
        );
        const password = env.get("HYPHA_OWNER_PASSWORD");
        const lines = [
          `API        ${url(deployment, deployment.domains.api)}`,
          ...(deployment.domains.admin
            ? [`Admin      ${url(deployment, deployment.domains.admin)}`]
            : []),
          ...(deployment.domains.storefront
            ? [`Storefront ${url(deployment, deployment.domains.storefront)}`]
            : []),
        ];
        if (password)
          lines.push(
            "",
            `Log in as ${env.get("HYPHA_OWNER_EMAIL")} with ${password}`,
            "(then change it)",
          );
        for (const entry of deployment.apps ?? [])
          lines.splice(lines.length, 0, `${entry.name.padEnd(10)} ${appUrl(deployment, entry)}`);
        p.note(lines.join("\n"), "Deployed");
        await offerInstalls(deployment, env, { quiet: args.yes });
        p.outro("Done.");
        return;
      }
      const { d, machine } = await open(dir);
      p.intro(`hypha deploy → ${machine.label}`);

      // Spores make the API the deployment's own: built from source, with its host crate.
      const spores = d.spores ?? [];
      const source = Boolean(args["build-from-source"] || d.source || spores.length);
      const building = source && !args["skip-build"];
      await preflight(d, machine, Boolean(args.yes), building);

      // The co-located apps: cloned, installed and bundled here, their images built
      // where the API's are.
      const coLocated: { entry: AppEntry; clone: string; profile: AppProfile }[] = [];
      for (const entry of d.apps ?? []) {
        const clone = cloneDir(resolve(dir), entry);
        if (!args["skip-build"]) {
          p.log.step(`Preparing the ${entry.name} app (${entry.rev!.slice(0, 12)})…`);
          await checkout(resolve(dir), entry);
        }
        const profile = await readProfile(clone);
        if (!profile.nodeBuild && !args["skip-build"]) {
          throw new Error(
            `the ${entry.name} app has no \`build:node\` script: it can't be built for a container`,
          );
        }
        coLocated.push({ entry, clone, profile });
      }

      if (building) {
        // Spores need a host crate of the deployment's own, git ones cloned here first.
        const hostCrate = spores.length
          ? await writeHostCrate(dir, spores, await fetchSpores(dir, spores))
          : undefined;
        for (const image of images(apps(d), hostCrate)) {
          p.log.step(
            `Building ${image.tag}${machine.ssh ? (d.buildOn === "server" ? " on the server" : " here") : ""}…`,
          );
          await build(d, machine, image);
        }
        for (const { entry, clone } of coLocated) {
          await run("pnpm", ["run", "build:node"], { cwd: clone, capture: true });
          const image = appImage(entry, clone);
          p.log.step(
            `Building ${image.tag}${machine.ssh ? (d.buildOn === "server" ? " on the server" : " here") : ""}…`,
          );
          await build(d, machine, image);
        }
      }
      await writeServerApps(resolve(dir), d, coLocated);
      {
        const env = await loadEnv(dir);
        const before = env.toString();
        setImageRefs(env, source, args.images);
        if (env.toString() !== before) await saveEnv(dir, env);
        if (!source)
          p.log.step(`Using the published images: ${publishedImage("api", args.images)}, …`);
      }

      const spin = p.spinner();
      spin.start("Copying the deployment and starting it");
      await machine.upload(dir, FILES);
      // The folder is root's alone; inside it, the services' containers read their files.
      if (machine.ssh)
        await machine.sh(`chmod 700 . && chmod 600 ${ENV_FILE} && chmod 644 seaweedfs/s3.json`);
      await machine.sh("docker compose up -d --remove-orphans", { capture: true });
      spin.message("Waiting for the API (the first time, Caddy is also getting certificates)");
      if (!(await waitForHealth(d, 300))) {
        spin.error("The API didn't answer");
        throw new Error(
          `${url(d, d.domains.api)}/health didn't answer in 5 minutes: see \`hypha logs api\` and \`hypha logs caddy\``,
        );
      }
      spin.stop("Running");

      const env = await loadEnv(dir);
      if (d.domains.storefront && env.get("STOREFRONT_KEY") && d.target === "vps") {
        // A changed domain moves the storefront's webhook with it.
        const email = env.get("HYPHA_OWNER_EMAIL");
        const password = env.get("HYPHA_OWNER_PASSWORD");
        if (email && password) {
          await Staff.login(d, email, password)
            .then((staff) => syncWebhook(d, staff))
            .then((moved) => moved && p.log.step(`The storefront's webhook now goes to ${moved}`))
            .catch((e) =>
              p.log.warn(
                `Couldn't check the storefront's webhook: ${e instanceof Error ? e.message : e}`,
              ),
            );
        }
      } else if (d.domains.storefront && !env.get("STOREFRONT_KEY")) {
        spin.start("Connecting the storefront: a publishable key and its webhook");
        const email = env.get("HYPHA_OWNER_EMAIL");
        const password = env.get("HYPHA_OWNER_PASSWORD");
        if (!email || !password) {
          spin.error("Skipped");
          p.log.warn(
            "No owner credentials in .env: set STOREFRONT_KEY and HYPHA_WEBHOOK_SECRET there, then deploy.",
          );
        } else {
          const keys = await connectStorefront(d, await Staff.login(d, email, password));
          env.set("STOREFRONT_KEY", keys.key).set("HYPHA_WEBHOOK_SECRET", keys.webhookSecret ?? "");
          await saveEnv(dir, env);
          await machine.upload(dir, [ENV_FILE]);
          await machine.sh("docker compose up -d storefront", { capture: true });
          spin.stop("Storefront connected");
          if (keys.orderUrl) p.log.step(`Order links in emails go to ${keys.orderUrl}`);
        }
      }

      for (const { entry } of coLocated) {
        const target = `${appUrl(d, entry)}/health`;
        if (!(await waitForAnswer(target, 120, d)))
          p.log.warn(
            `The ${entry.name} app doesn't answer at ${target}: see \`hypha logs ${service(entry)}\``,
          );
      }

      const lines = [
        `API        ${url(d, d.domains.api)}`,
        ...(d.domains.admin ? [`Admin      ${url(d, d.domains.admin)}`] : []),
        ...(d.domains.storefront ? [`Storefront ${url(d, d.domains.storefront)}`] : []),
        ...coLocated.map(({ entry }) => `${entry.name.padEnd(10)} ${appUrl(d, entry)}`),
      ];
      const password = env.get("HYPHA_OWNER_PASSWORD");
      if (password)
        lines.push(
          "",
          `Log in as ${env.get("HYPHA_OWNER_EMAIL")} with ${password}`,
          "(then change it)",
        );
      p.note(lines.join("\n"), "Deployed");
      await offerInstalls(d, env, { quiet: args.yes });
      p.outro("Done.");
    }),
});

/**
 * The co-located apps' part of a server's files: `compose.apps.yaml` (their containers,
 * read after `compose.yaml` through `.env`'s COMPOSE_FILE), `apps.caddy` (their sites,
 * which the Caddyfile imports), their encryption keys in `.env`, and on this machine
 * calls to private addresses allowed (the apps are other containers). Written on every
 * deploy, with no apps too, so a removed app's container goes (`--remove-orphans`).
 */
export async function writeServerApps(
  dir: string,
  d: Deployment,
  coLocated: { entry: AppEntry; profile: AppProfile }[],
): Promise<void> {
  await writeFile(join(dir, COMPOSE_APPS_FILE), composeApps(d, coLocated));
  await writeFile(join(dir, APPS_CADDY_FILE), appsCaddy(d, coLocated));
  const caddyfile = await readFile(join(dir, "Caddyfile"), "utf8");
  if (!caddyfile.split("\n").includes(CADDY_IMPORT)) {
    // A Caddyfile from before the apps: the import goes at its end, the rest as it was.
    await writeFile(
      join(dir, "Caddyfile"),
      `${caddyfile.trimEnd()}\n\n# The co-located apps' sites, written by \`hypha deploy\`.\n${CADDY_IMPORT}\n`,
    );
  }
  const env = await loadEnv(dir);
  const before = env.toString();
  env.set("COMPOSE_FILE", `compose.yaml:${COMPOSE_APPS_FILE}`).set("COMPOSE_PATH_SEPARATOR", ":");
  // The API's public address (HYPHA_PUBLIC_URL in compose.yaml), for a `.env` from
  // before it, and after the domain changed in hypha.json.
  env.set("API_URL", url(d, d.domains.api));
  ensureEncryptionKeys(d, env);
  if (d.target === "local" && coLocated.length > 0)
    env.set("HYPHA_ALLOW_PRIVATE_CALLBACKS", "true");
  if (env.toString() !== before) await saveEnv(dir, env);
}

/** Checks the server before anything is built: SSH, Docker (offering to install it),
 * and that the domains point at it. */
async function preflight(
  d: Deployment,
  machine: Machine,
  yes: boolean,
  building: boolean,
): Promise<void> {
  if (!machine.ssh) {
    await machine.sh("docker compose version", { capture: true }).catch(() => {
      throw new Error("Docker with Compose isn't running here: start Docker Desktop");
    });
    const bytes = Number(
      (await machine.sh("docker info --format '{{.MemTotal}}'", { capture: true })).trim(),
    );
    if (building && bytes < BUILD_MEMORY_MB * 1024 * 1024) {
      throw new Error(
        `Docker has ${(bytes / 2 ** 30).toFixed(1)} GiB of memory; compiling the API needs about 5: ` +
          "raise it in Docker Desktop (Settings → Resources → Memory), then deploy again",
      );
    }
    return;
  }

  try {
    await machine.sh("true", { capture: true });
  } catch (e) {
    throw new Error(
      `can't reach ${machine.ssh} over SSH without a password (key-based login is needed):\n` +
        (e instanceof CommandFailed ? e.output.trim() : String(e)),
      { cause: e },
    );
  }

  const docker = await machine.sh("docker compose version", { capture: true }).then(
    () => true,
    () => false,
  );
  if (!docker) {
    const install =
      yes ||
      (process.stdin.isTTY &&
        (await p.confirm({
          message: `Docker isn't installed on ${machine.ssh}. Install it with Docker's script (get.docker.com)?`,
        })) === true);
    if (!install)
      throw new Error("Docker with Compose is needed on the server: install it, then deploy again");
    p.log.step("Installing Docker…");
    await machine.sh("curl -fsSL https://get.docker.com | sh");
  }

  if (building && d.buildOn === "server") await ensureBuildMemory(machine, yes);

  // Caddy can only get a certificate for a domain that leads here.
  const host = machine.ssh.split("@").pop()!;
  const serverIps = isIP(host) ? [host] : await addresses(host);
  const wrong: string[] = [];
  for (const domain of [
    ...Object.values(d.domains),
    ...(d.apps ?? []).map((a) => appDomain(d, a)),
  ]) {
    if (!domain) continue;
    const ips = await addresses(domain);
    if (!ips.some((ip) => serverIps.includes(ip)))
      wrong.push(`${domain} → ${ips.join(", ") || "nothing"}`);
  }
  if (wrong.length > 0) {
    p.log.warn(`These domains don't point at ${serverIps.join(", ")} (yet?):\n${wrong.join("\n")}`);
    const go =
      yes ||
      (process.stdin.isTTY &&
        (await p.confirm({ message: "Deploy anyway?", initialValue: false })) === true);
    if (!go) throw new Error("fix the DNS records (or wait for them to spread), then deploy again");
  }
}

/** Memory and swap the API's compile needs: its last crate peaks near 3 GB, on top of
 * Docker and the system. A "4 GB" server reports about 3.8, so it gets swap. */
const BUILD_MEMORY_MB = 5120;

/** On a server with less memory than compiling needs, offers a swap file (slower, but
 * it finishes) or building here instead. */
async function ensureBuildMemory(machine: Machine, yes: boolean): Promise<void> {
  const meminfo = await machine.sh("cat /proc/meminfo", { capture: true });
  const kb = (name: string) =>
    Number(new RegExp(`^${name}:\\s+(\\d+)`, "m").exec(meminfo)?.[1] ?? 0);
  const totalMb = Math.floor((kb("MemTotal") + kb("SwapTotal")) / 1024);
  if (totalMb >= BUILD_MEMORY_MB) return;
  const swapMb = Math.ceil((BUILD_MEMORY_MB - totalMb) / 1024) * 1024 + 1024;
  p.log.warn(
    `The server has ${totalMb} MB of memory and swap; compiling the API needs about ${BUILD_MEMORY_MB}.`,
  );
  const add =
    yes ||
    (process.stdin.isTTY &&
      (await p.confirm({
        message: `Add a ${swapMb / 1024} GB swap file (/swapfile, kept across reboots)?`,
      })) === true);
  if (!add)
    throw new Error(
      "not enough memory to compile there: add swap, or `--build-on here` in `hypha init`",
    );
  await machine.sh(
    [
      `fallocate -l ${swapMb}M /swapfile`,
      "chmod 600 /swapfile",
      "mkswap /swapfile",
      "swapon /swapfile",
      "grep -q '^/swapfile ' /etc/fstab || echo '/swapfile none swap sw 0 0' >> /etc/fstab",
    ].join(" && "),
    { capture: true },
  );
}

async function addresses(host: string): Promise<string[]> {
  const [v4, v6] = await Promise.all([
    resolve4(host).catch(() => []),
    resolve6(host).catch(() => []),
  ]);
  return [...v4, ...v6];
}
