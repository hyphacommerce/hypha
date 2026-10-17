// `hypha init`: asks where Hypha runs and under which domains, and writes the
// deployment's folder. Every question has a flag, so it also runs unattended (an
// agent, a script): with `--yes`, what isn't given takes its default.

import { randomInt } from "node:crypto";
import { chmod, mkdir, stat, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";

import * as p from "@clack/prompts";
import { defineCommand } from "citty";

import {
  type BuildOn,
  type Deployment,
  ENV_FILE,
  loadDeployment,
  loadEnv,
  saveDeployment,
  saveEnv,
  STATE_FILE,
  type Target,
  url,
} from "../deployment.ts";
import { checkDatabaseUrl } from "../cloudflare.ts";
import {
  caddyfile,
  composeFile,
  configSchema,
  configToml,
  newCloudflareEnv,
  newEnv,
  seaweedConfig,
} from "../templates.ts";
import { ask, dirArg, guarded, setUnattended } from "./shared.ts";

/** What `--force` keeps from the deployment's `.env`. */
const KEPT = [
  "POSTGRES_PASSWORD",
  "HYPHA_SECRET_KEY",
  "S3_ACCESS_KEY",
  "S3_SECRET_KEY",
  "HYPHA_OWNER_EMAIL",
  "HYPHA_OWNER_PASSWORD",
  "STOREFRONT_KEY",
  "HYPHA_WEBHOOK_SECRET",
  "DATABASE_URL",
  "R2_ACCESS_KEY_ID",
  "R2_SECRET_ACCESS_KEY",
];

/** A Cloudflare deployment's name: its resources' prefix. */
const NAME = /^[a-z][a-z0-9-]{0,40}[a-z0-9]$/;

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
    target: {
      type: "string",
      description: "vps (a server over SSH), cloudflare (the API on Workers) or local (this machine, to try it)",
    },
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
    "acme-email": { type: "string", description: "For Let's Encrypt's expiry notices (default the owner's; none for none)" },
    "build-on": { type: "string", description: "server (default) or here: where the images are built" },
    "remote-dir": { type: "string", default: "/opt/hypha", description: "The deployment's folder on the server" },
    "database-url": {
      type: "string",
      description: "cloudflare: the Postgres database's direct URL (any provider; not a pooler's)",
    },
    name: { type: "string", description: "cloudflare: its resources' prefix (default hypha: hypha-api, hypha-media, …)" },
    "account-id": { type: "string", description: "cloudflare: the account, when wrangler reaches more than one" },
    "zone-id": { type: "string", description: "cloudflare: the media domain's zone, for the bucket's own domain" },
    region: {
      type: "string",
      description: "cloudflare: a placement region (aws:eu-central-1) instead of measuring the database's host",
    },
    "r2-access-key-id": { type: "string", description: "cloudflare: an R2 API token's access key ID (media is off without)" },
    "r2-secret-access-key": { type: "string", description: "cloudflare: that token's secret access key" },
    yes: { type: "boolean", alias: "y", description: "Don't ask: defaults for what isn't given" },
    force: { type: "boolean", description: "Write an existing deployment again (keeps its secrets, owner and keys)" },
  },
  run: ({ args }) =>
    guarded(async () => {
    const dir = resolve(args.dir);
    setUnattended(Boolean(args.yes));
    p.intro("hypha init");

    if (!args.force && (await exists(join(dir, STATE_FILE)))) {
      p.cancel(`${dir} already holds a deployment: \`hypha deploy\` it, or --force to write it again (keeps its secrets)`);
      process.exit(1);
    }

    const target = (await ask(args.target, "vps", () =>
      p.select<Target>({
        message: "Where should Hypha run?",
        options: [
          { value: "vps", label: "A server", hint: "a VPS you reach over SSH (Hetzner, DigitalOcean, …)" },
          { value: "cloudflare", label: "Cloudflare", hint: "the API on Workers, with your Postgres (Neon, Supabase, …)" },
          { value: "local", label: "This machine", hint: "to try the production setup locally" },
        ],
      }),
    )) as Target;
    if (target !== "vps" && target !== "local" && target !== "cloudflare") {
      throw new Error(`--target is vps, cloudflare or local, not ${target}`);
    }
    if (target === "cloudflare") return initCloudflare(dir, args);

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
    const env = newEnv(d, { email, storeName });
    // Written again (new domains, say): the server's data was made with these, so they
    // stay, and so do the owner and the storefront's keys.
    const earlier = args.force ? await loadEnv(dir).catch(() => undefined) : undefined;
    if (earlier) for (const key of KEPT) {
      const value = earlier.get(key);
      if (value !== undefined && value !== "" && env.get(key) !== undefined) env.set(key, value);
    }
    const acme = args["acme-email"] === "none" ? undefined : (args["acme-email"] || email);

    await mkdir(join(dir, "seaweedfs"), { recursive: true, mode: 0o700 });
    await chmod(dir, 0o700);
    await saveDeployment(dir, d);
    await saveEnv(dir, env);
    await writeFile(join(dir, "compose.yaml"), await composeFile());
    await writeFile(join(dir, "Caddyfile"), caddyfile(d, acme));
    await writeFile(join(dir, "hypha.config.toml"), configToml());
    await writeFile(join(dir, "hypha.config.schema.json"), await configSchema());
    await writeFile(join(dir, "seaweedfs", "s3.json"), seaweedConfig(env.get("S3_ACCESS_KEY")!, env.get("S3_SECRET_KEY")!));
    // SeaweedFS reads it as its own user; the folder (700) is what keeps it private.
    await chmod(join(dir, "seaweedfs", "s3.json"), 0o644);
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

/** The flags `initCloudflare` reads. */
type CloudflareArgs = {
  dir: string;
  force?: boolean;
  domain?: string;
  "api-domain"?: string;
  "admin-domain"?: string;
  "media-domain"?: string;
  admin?: boolean;
  "owner-email"?: string;
  "store-name"?: string;
  "database-url"?: string;
  name?: string;
  "account-id"?: string;
  "zone-id"?: string;
  region?: string;
  "r2-access-key-id"?: string;
  "r2-secret-access-key"?: string;
};

/**
 * `--target cloudflare` (design note 0026, §5): the API as a Worker on your account,
 * over Hyperdrive to the Postgres you give (any provider). Only the API for now: the
 * admin's domain is where invite links and uploads come from, hosted where you like.
 */
async function initCloudflare(dir: string, args: CloudflareArgs): Promise<void> {
  const databaseUrl = (
    await ask(args["database-url"], undefined, () =>
      p.text({
        message: "The Postgres database's direct URL (Neon, Supabase, RDS, your own; not a pooler's)",
        placeholder: "postgres://user:password@host:5432/hypha?sslmode=require",
        validate: checkDatabaseUrl,
      }),
    )
  ).trim();
  const problem = checkDatabaseUrl(databaseUrl);
  if (problem) throw new Error(`--database-url: ${problem}`);

  const domain = (
    await ask(args.domain, undefined, () =>
      p.text({ message: "Your domain, on this Cloudflare account", placeholder: "example.com", validate: checkDomain }),
    )
  ).toLowerCase();
  if (checkDomain(domain)) throw new Error(`--domain: ${domain} isn't a domain`);
  const sub = async (flag: string | undefined, label: string, fallback: string) => {
    const value = await ask(flag, fallback, () => p.text({ message: label, initialValue: fallback, validate: checkDomain }));
    if (checkDomain(value)) throw new Error(`${label}: ${value} isn't a domain`);
    return value.toLowerCase();
  };
  const domains: Deployment["domains"] = {
    api: await sub(args["api-domain"], "The API's address", `api.${domain}`),
    media: await sub(args["media-domain"], "Uploaded files' address", `media.${domain}`),
    ...(args.admin ? { admin: await sub(args["admin-domain"], "The admin's address", `admin.${domain}`) } : {}),
  };

  const name = await ask(args.name, "hypha", () =>
    p.text({
      message: "A name for its resources (the Worker <name>-api, the bucket <name>-media, …)",
      initialValue: "hypha",
      validate: (v) => (v && NAME.test(v) ? undefined : "lowercase letters, digits and dashes"),
    }),
  );
  if (!NAME.test(name)) throw new Error(`--name: ${name} should be lowercase letters, digits and dashes`);
  const zoneId = await ask(args["zone-id"], "", () =>
    p.text({
      message: `The zone ID of ${domain}, for files on ${domains.media} (Overview page; empty for signed URLs)`,
      defaultValue: "",
    }),
  );
  const r2Key = await ask(args["r2-access-key-id"], "", () =>
    p.text({
      message: "An R2 API token's access key ID, for files (R2 → Manage API tokens; empty for no media)",
      defaultValue: "",
    }),
  );
  const r2Secret = r2Key
    ? await ask(args["r2-secret-access-key"], undefined, () =>
        p.password({ message: "Its secret access key", validate: (v) => (v ? undefined : "the token's secret") }),
      )
    : "";

  const email = (
    await ask(args["owner-email"], undefined, () =>
      p.text({ message: "The owner's email (the first staff user)", validate: checkEmail }),
    )
  ).trim();
  if (checkEmail(email)) throw new Error(`--owner-email: ${email} isn't an email address`);
  const storeName = await ask(args["store-name"], "My store", () =>
    p.text({ message: "The store's name", initialValue: "My store" }),
  );

  // Written again: keep what Cloudflare already holds (the resources' IDs, the account).
  const earlierState = args.force ? await loadDeployment(dir).catch(() => undefined) : undefined;
  const earlier = earlierState?.cloudflare?.name === name ? earlierState.cloudflare : undefined;
  const d: Deployment = {
    version: 1,
    target: "cloudflare",
    remoteDir: "",
    buildOn: "here",
    scheme: "https",
    domains,
    cloudflare: {
      name,
      ...((args["account-id"] || earlier?.accountId) ? { accountId: args["account-id"] || earlier?.accountId } : {}),
      ...(zoneId.trim() ? { zoneId: zoneId.trim() } : {}),
      ...(args.region ? { region: args.region } : {}),
      // A namespace_id is a number unique per account: a random block of ten.
      rateLimitBase: earlier?.rateLimitBase ?? 1000 + randomInt(90_000) * 10,
      ...(earlier?.hyperdriveId ? { hyperdriveId: earlier.hyperdriveId } : {}),
      ...(earlier?.kvId ? { kvId: earlier.kvId } : {}),
    },
  };
  const env = newCloudflareEnv({ email, storeName }, databaseUrl, { id: r2Key.trim(), secret: r2Secret.trim() });
  const earlierEnv = args.force ? await loadEnv(dir).catch(() => undefined) : undefined;
  // The database URL and R2 keys given now win; the secret key and owner stay.
  const given = new Set(["DATABASE_URL", ...(r2Key ? ["R2_ACCESS_KEY_ID", "R2_SECRET_ACCESS_KEY"] : [])]);
  if (earlierEnv) for (const key of KEPT) {
    const value = earlierEnv.get(key);
    if (!given.has(key) && value !== undefined && value !== "" && env.get(key) !== undefined) env.set(key, value);
  }

  await mkdir(dir, { recursive: true, mode: 0o700 });
  await chmod(dir, 0o700);
  await saveDeployment(dir, d);
  await saveEnv(dir, env);
  await writeFile(join(dir, "hypha.config.toml"), configToml("cloudflare"));
  await writeFile(join(dir, "hypha.config.schema.json"), await configSchema());
  await writeFile(join(dir, ".gitignore"), `# Secrets: never commit these.\n${ENV_FILE}\n.secrets.json\n.cors.json\n`);

  p.note(
    [
      `API        ${url(d, domains.api)}`,
      `Files      ${r2Key ? (zoneId ? url(d, domains.media) : "signed URLs from R2") : "off (no R2 keys)"}`,
      ...(domains.admin ? [`Admin      ${url(d, domains.admin)} (host it where you like, for now)`] : []),
      `Database   ${new URL(databaseUrl).host}`,
    ].join("\n"),
    `Written to ${dir}`,
  );
  p.log.step("hypha deploy needs `wrangler login`, Rust with `rustup target add wasm32-unknown-unknown`,\nand `cargo install worker-build`.");
  p.outro(`Next: hypha deploy${args.dir === "hypha-deploy" ? "" : ` --dir ${args.dir}`}`);
}
