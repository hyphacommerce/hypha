// `hypha deploy`: checks the server, builds the images, copies the deployment's files,
// starts the stack and waits for it; the first time, also connects the storefront.
// Run it again after any change (code, `.env`, `hypha.config.toml`): it's the same
// steps, and Compose restarts only what changed.

import { resolve4, resolve6 } from "node:dns/promises";
import { isIP } from "node:net";

import * as p from "@clack/prompts";
import { defineCommand } from "citty";

import { Staff, waitForHealth } from "../api.ts";
import { apps, type Deployment, ENV_FILE, loadEnv, saveEnv, url } from "../deployment.ts";
import { build, images } from "../images.ts";
import { CommandFailed, type Machine } from "../shell.ts";
import { connectStorefront } from "../storefront.ts";
import { dirArg, guarded, open } from "./shared.ts";

/** What the server runs: copied on every deploy. */
const FILES = [ENV_FILE, "compose.yaml", "Caddyfile", "hypha.config.toml", "hypha.config.schema.json", "seaweedfs/s3.json"];

export const deploy = defineCommand({
  meta: { name: "deploy", description: "Build, copy and start the deployment (again after any change)" },
  args: {
    dir: dirArg,
    "skip-build": { type: "boolean", description: "Use the images already there" },
    yes: { type: "boolean", alias: "y", description: "Don't ask (installing Docker, DNS warnings)" },
  },
  run: ({ args }) =>
    guarded(async () => {
      const dir = args.dir;
      const { d, machine } = await open(dir);
      p.intro(`hypha deploy → ${machine.label}`);

      await preflight(d, machine, Boolean(args.yes), !args["skip-build"]);

      if (!args["skip-build"]) {
        for (const image of images(apps(d))) {
          p.log.step(`Building ${image.tag}${machine.ssh ? (d.buildOn === "server" ? " on the server" : " here") : ""}…`);
          await build(d, machine, image);
        }
      }

      const spin = p.spinner();
      spin.start("Copying the deployment and starting it");
      await machine.upload(dir, FILES);
      if (machine.ssh) await machine.sh(`chmod 600 ${ENV_FILE} seaweedfs/s3.json`);
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
      if (d.domains.storefront && !env.get("STOREFRONT_KEY")) {
        spin.start("Connecting the storefront: a publishable key and its webhook");
        const email = env.get("HYPHA_OWNER_EMAIL");
        const password = env.get("HYPHA_OWNER_PASSWORD");
        if (!email || !password) {
          spin.error("Skipped");
          p.log.warn("No owner credentials in .env: set STOREFRONT_KEY and HYPHA_WEBHOOK_SECRET there, then deploy.");
        } else {
          const keys = await connectStorefront(d, await Staff.login(d, email, password));
          env.set("STOREFRONT_KEY", keys.key).set("HYPHA_WEBHOOK_SECRET", keys.webhookSecret ?? "");
          await saveEnv(dir, env);
          await machine.upload(dir, [ENV_FILE]);
          await machine.sh("docker compose up -d storefront", { capture: true });
          spin.stop("Storefront connected");
        }
      }

      const lines = [
        `API        ${url(d, d.domains.api)}`,
        ...(d.domains.admin ? [`Admin      ${url(d, d.domains.admin)}`] : []),
        ...(d.domains.storefront ? [`Storefront ${url(d, d.domains.storefront)}`] : []),
      ];
      const password = env.get("HYPHA_OWNER_PASSWORD");
      if (password) lines.push("", `Log in as ${env.get("HYPHA_OWNER_EMAIL")} with ${password}`, "(then change it)");
      p.note(lines.join("\n"), "Deployed");
      p.outro("Done.");
    }),
});

/** Checks the server before anything is built: SSH, Docker (offering to install it),
 * and that the domains point at it. */
async function preflight(d: Deployment, machine: Machine, yes: boolean, building: boolean): Promise<void> {
  if (!machine.ssh) {
    await machine.sh("docker compose version", { capture: true }).catch(() => {
      throw new Error("Docker with Compose isn't running here: start Docker Desktop");
    });
    const bytes = Number((await machine.sh("docker info --format '{{.MemTotal}}'", { capture: true })).trim());
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
    if (!install) throw new Error("Docker with Compose is needed on the server: install it, then deploy again");
    p.log.step("Installing Docker…");
    await machine.sh("curl -fsSL https://get.docker.com | sh");
  }

  if (building && d.buildOn === "server") await ensureBuildMemory(machine, yes);

  // Caddy can only get a certificate for a domain that leads here.
  const host = machine.ssh.split("@").pop()!;
  const serverIps = isIP(host) ? [host] : await addresses(host);
  const wrong: string[] = [];
  for (const domain of Object.values(d.domains)) {
    if (!domain) continue;
    const ips = await addresses(domain);
    if (!ips.some((ip) => serverIps.includes(ip))) wrong.push(`${domain} → ${ips.join(", ") || "nothing"}`);
  }
  if (wrong.length > 0) {
    p.log.warn(`These domains don't point at ${serverIps.join(", ")} (yet?):\n${wrong.join("\n")}`);
    const go = yes || (process.stdin.isTTY && (await p.confirm({ message: "Deploy anyway?", initialValue: false })) === true);
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
  const kb = (name: string) => Number(new RegExp(`^${name}:\\s+(\\d+)`, "m").exec(meminfo)?.[1] ?? 0);
  const totalMb = Math.floor((kb("MemTotal") + kb("SwapTotal")) / 1024);
  if (totalMb >= BUILD_MEMORY_MB) return;
  const swapMb = Math.ceil((BUILD_MEMORY_MB - totalMb) / 1024) * 1024 + 1024;
  p.log.warn(`The server has ${totalMb} MB of memory and swap; compiling the API needs about ${BUILD_MEMORY_MB}.`);
  const add =
    yes ||
    (process.stdin.isTTY &&
      (await p.confirm({ message: `Add a ${swapMb / 1024} GB swap file (/swapfile, kept across reboots)?` })) === true);
  if (!add) throw new Error("not enough memory to compile there: add swap, or `--build-on here` in `hypha init`");
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
  const [v4, v6] = await Promise.all([resolve4(host).catch(() => []), resolve6(host).catch(() => [])]);
  return [...v4, ...v6];
}
