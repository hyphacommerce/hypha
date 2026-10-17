// `hypha init`: asks where Hypha runs and under which domains, and writes the
// deployment's folder. Every question has a flag, so it also runs unattended (an
// agent, a script): with `--yes`, what isn't given takes its default.

import { mkdir, stat, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";

import * as p from "@clack/prompts";
import { defineCommand } from "citty";

import {
  type BuildOn,
  type Deployment,
  ENV_FILE,
  saveDeployment,
  saveEnv,
  STATE_FILE,
  type Target,
  url,
} from "../deployment.ts";
import { caddyfile, composeFile, configSchema, configToml, newEnv, seaweedConfig } from "../templates.ts";
import { ask, dirArg, guarded, setUnattended } from "./shared.ts";

const DOMAIN = /^(?=.{1,253}$)([a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z][a-z0-9-]{0,62}$/i;

function checkDomain(value: string | undefined): string | undefined {
  return value && DOMAIN.test(value.trim()) ? undefined : "a domain like shop.example.com";
}

function checkEmail(value: string | undefined): string | undefined {
  return value && /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(value.trim()) ? undefined : "an email address";
}

export const init = defineCommand({
  meta: { name: "init", description: "Write a deployment: where Hypha runs, its domains, its secrets" },
  args: {
    dir: dirArg,
    target: { type: "string", description: "vps (a server over SSH) or local (this machine, to try it)" },
    ssh: { type: "string", description: "The server's SSH destination, e.g. root@203.0.113.7" },
    domain: { type: "string", description: "Your domain: api., admin. and media. go under it, the shop on it" },
    "api-domain": { type: "string", description: "The API's domain (default api.<domain>)" },
    "admin-domain": { type: "string", description: "The admin's domain (default admin.<domain>)" },
    "media-domain": { type: "string", description: "Uploaded files' domain (default media.<domain>)" },
    "storefront-domain": { type: "string", description: "The shop's domain (default <domain>)" },
    admin: { type: "boolean", default: true, description: "Include the admin (--no-admin to host your own)" },
    storefront: { type: "boolean", default: true, description: "Include the storefront (--no-storefront for your own)" },
    "owner-email": { type: "string", description: "The first staff user, the store's owner" },
    "store-name": { type: "string", description: "The store's name" },
    "acme-email": { type: "string", description: "For Let's Encrypt's expiry notices (default the owner's)" },
    "build-on": { type: "string", description: "server (default) or here: where the images are built" },
    "remote-dir": { type: "string", default: "/opt/hypha", description: "The deployment's folder on the server" },
    yes: { type: "boolean", alias: "y", description: "Don't ask: defaults for what isn't given" },
    force: { type: "boolean", description: "Overwrite an existing deployment (new secrets!)" },
  },
  run: ({ args }) =>
    guarded(async () => {
    const dir = resolve(args.dir);
    setUnattended(Boolean(args.yes));
    p.intro("hypha init");

    if (!args.force && (await exists(join(dir, STATE_FILE)))) {
      p.cancel(`${dir} already holds a deployment: \`hypha deploy\` it, or --force to start over (new secrets)`);
      process.exit(1);
    }

    const target = (await ask(args.target, "vps", () =>
      p.select<Target>({
        message: "Where should Hypha run?",
        options: [
          { value: "vps", label: "A server", hint: "a VPS you reach over SSH (Hetzner, DigitalOcean, …)" },
          { value: "local", label: "This machine", hint: "to try the production setup locally" },
        ],
      }),
    )) as Target;
    if (target !== "vps" && target !== "local") throw new Error(`--target is vps or local, not ${target}`);

    let ssh: string | undefined;
    let domains: Deployment["domains"];
    let buildOn: BuildOn = "server";
    if (target === "vps") {
      ssh = await ask(args.ssh, undefined, () =>
        p.text({
          message: "The server's SSH destination",
          placeholder: "root@203.0.113.7",
          validate: (v) => (v?.trim() ? undefined : "user@host, as you'd pass it to ssh"),
        }),
      );
      const domain = (
        await ask(args.domain, undefined, () =>
          p.text({ message: "Your domain", placeholder: "example.com", validate: checkDomain }),
        )
      ).toLowerCase();
      if (checkDomain(domain)) throw new Error(`--domain: ${domain} isn't a domain`);
      const sub = async (flag: string | undefined, label: string, fallback: string) => {
        const value = await ask(flag, fallback, () =>
          p.text({ message: label, initialValue: fallback, validate: checkDomain }),
        );
        if (checkDomain(value)) throw new Error(`${label}: ${value} isn't a domain`);
        return value.toLowerCase();
      };
      domains = {
        api: await sub(args["api-domain"], "The API's address", `api.${domain}`),
        media: await sub(args["media-domain"], "Uploaded files' address", `media.${domain}`),
        ...(args.admin ? { admin: await sub(args["admin-domain"], "The admin's address", `admin.${domain}`) } : {}),
        ...(args.storefront
          ? { storefront: await sub(args["storefront-domain"], "The shop's address", domain) }
          : {}),
      };
      buildOn = (await ask(args["build-on"], "server", () =>
        p.select<BuildOn>({
          message: "Where should the images be built?",
          options: [
            { value: "server", label: "On the server", hint: "nothing to copy; compiling needs ~4 GB of memory" },
            { value: "here", label: "Here", hint: "then copied over; for small servers" },
          ],
        }),
      )) as BuildOn;
      if (buildOn !== "server" && buildOn !== "here") throw new Error(`--build-on is server or here, not ${buildOn}`);
    } else {
      domains = {
        api: "api.hypha.localhost",
        media: "media.hypha.localhost",
        ...(args.admin ? { admin: "admin.hypha.localhost" } : {}),
        ...(args.storefront ? { storefront: "shop.hypha.localhost" } : {}),
      };
    }

    const email = (
      await ask(args["owner-email"], undefined, () =>
        p.text({ message: "The owner's email (the first staff user)", validate: checkEmail }),
      )
    ).trim();
    if (checkEmail(email)) throw new Error(`--owner-email: ${email} isn't an email address`);
    const storeName = await ask(args["store-name"], "My store", () =>
      p.text({ message: "The store's name", initialValue: "My store" }),
    );

    const d: Deployment = {
      version: 1,
      target,
      ...(ssh ? { ssh: ssh.trim() } : {}),
      remoteDir: args["remote-dir"],
      buildOn,
      scheme: target === "vps" ? "https" : "http",
      domains,
    };
    const env = newEnv(d, { email, storeName, acmeEmail: args["acme-email"] });

    await mkdir(join(dir, "seaweedfs"), { recursive: true });
    await saveDeployment(dir, d);
    await saveEnv(dir, env);
    await writeFile(join(dir, "compose.yaml"), await composeFile());
    await writeFile(join(dir, "Caddyfile"), caddyfile(d));
    await writeFile(join(dir, "hypha.config.toml"), configToml());
    await writeFile(join(dir, "hypha.config.schema.json"), await configSchema());
    await writeFile(join(dir, "seaweedfs", "s3.json"), seaweedConfig(env.get("S3_ACCESS_KEY")!, env.get("S3_SECRET_KEY")!), {
      mode: 0o600,
    });
    await writeFile(join(dir, ".gitignore"), `# Secrets: never commit these.\n${ENV_FILE}\nseaweedfs/\n`);

    const lines = [
      `API        ${url(d, domains.api)}`,
      ...(domains.admin ? [`Admin      ${url(d, domains.admin)}`] : []),
      ...(domains.storefront ? [`Storefront ${url(d, domains.storefront)}`] : []),
      `Files      ${url(d, domains.media)}`,
    ];
    p.note(lines.join("\n"), `Written to ${dir}`);
    if (target === "vps") {
      const host = ssh!.split("@").pop();
      p.log.step(
        `Point these domains at the server (A/AAAA records to ${host}) before deploying,\n` +
          "so Caddy can get their certificates.",
      );
    }
    p.outro(`Next: hypha deploy${args.dir === "hypha-deploy" ? "" : ` --dir ${args.dir}`}`);
    }),
});

async function exists(path: string): Promise<boolean> {
  try {
    await stat(path);
    return true;
  } catch {
    return false;
  }
}
