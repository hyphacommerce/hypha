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
import { checkR2Keys, checkToken, isTokenValue, MANUAL, r2KeysOf, templateUrl } from "../cloudflare-token.ts";
import { BLOCK, checkDatabaseUrl } from "../cloudflare.ts";
import { run } from "../shell.ts";
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
  "CLOUDFLARE_API_TOKEN",
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
    "api-token": {
      type: "string",
      description: "cloudflare: the API token to deploy with (else CLOUDFLARE_API_TOKEN, else init links to a filled-in form)",
    },
    "zone-id": { type: "string", description: "cloudflare: the domain's zone (found from the token otherwise)" },
    region: {
      type: "string",
      description: "cloudflare: a placement region (aws:eu-central-1) instead of measuring the database's host",
    },
    "r2-access-key-id": {
      type: "string",
      description: "cloudflare: R2's Access Key ID, or an R2 token's value (default: derived from the API token)",
    },
    "r2-secret-access-key": { type: "string", description: "cloudflare: R2's Secret Access Key, with an Access Key ID" },
    media: { type: "boolean", default: true, description: "cloudflare: files on R2 (--no-media for none)" },
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
  yes?: boolean;
  domain?: string;
  "api-domain"?: string;
  "admin-domain"?: string;
  "media-domain"?: string;
  "storefront-domain"?: string;
  admin?: boolean;
  storefront?: boolean;
  "owner-email"?: string;
  "store-name"?: string;
  "database-url"?: string;
  name?: string;
  "api-token"?: string;
  "account-id"?: string;
  "zone-id"?: string;
  region?: string;
  "r2-access-key-id"?: string;
  "r2-secret-access-key"?: string;
  media?: boolean;
};

/**
 * `--target cloudflare` (design note 0026, §5): the API, the admin and the storefront as
 * Workers on your account, the API over Hyperdrive to the Postgres you give (any
 * provider), all deployed with one API token.
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
      p.text({ message: "Your domain, on Cloudflare", placeholder: "example.com", validate: checkDomain }),
    )
  ).toLowerCase();
  if (checkDomain(domain)) throw new Error(`--domain: ${domain} isn't a domain`);
  const sub = async (flag: string | undefined, label: string, fallback: string) => {
    const value = await ask(flag, fallback, () => p.text({ message: label, initialValue: fallback, validate: checkDomain }));
    if (checkDomain(value)) throw new Error(`${label}: ${value} isn't a domain`);
    return value.toLowerCase();
  };
  // Each becomes a Worker's custom domain: none may have a DNS record already.
  const domains: Deployment["domains"] = {
    api: await sub(args["api-domain"], "The API's address", `api.${domain}`),
    media: await sub(args["media-domain"], "Uploaded files' address", `media.${domain}`),
    ...(args.admin ? { admin: await sub(args["admin-domain"], "The admin's address", `admin.${domain}`) } : {}),
    ...(args.storefront
      ? { storefront: await sub(args["storefront-domain"], "The shop's address", `shop.${domain}`) }
      : {}),
  };

  const name = await ask(args.name, "hypha", () =>
    p.text({
      message: "A name for its resources (the Workers <name>-api, -admin, -storefront, …)",
      initialValue: "hypha",
      validate: (v) => (v && NAME.test(v) ? undefined : "lowercase letters, digits and dashes"),
    }),
  );
  if (!NAME.test(name)) throw new Error(`--name: ${name} should be lowercase letters, digits and dashes`);
  // Files: R2 keys by flag (an R2 token's value is turned into them), else the ones
  // `--force` keeps, else derived from the deploy token if it may use R2.
  const r2Flag = (args["r2-access-key-id"] ?? "").trim();
  const earlierEnv = args.force ? await loadEnv(dir).catch(() => undefined) : undefined;
  const keptR2 = earlierEnv?.get("R2_ACCESS_KEY_ID") ? earlierEnv : undefined;
  const wantsMedia = args.media !== false;

  // The token everything deploys with (cloudflare-token.ts).
  const knownToken = earlierEnv?.get("CLOUDFLARE_API_TOKEN") || process.env.CLOUDFLARE_API_TOKEN || undefined;
  if (!args["api-token"] && !knownToken) {
    const link = templateUrl(`Hypha ${name}`);
    p.log.step(
      "Hypha deploys with a Cloudflare API token. Create one with this link (most permissions are\n" +
        `filled in; pick the account and ${domain}'s zone), add these with "+ Add more", which the\n` +
        `link can't fill in:\n${MANUAL.map((m) => `  ${m}`).join("\n")}\nthen paste the token here:\n${link}`,
    );
    if (process.stdin.isTTY && !args.yes) openInBrowser(link);
  }
  const token = (
    await ask(args["api-token"], knownToken, () =>
      p.password({ message: "The API token", validate: (v) => (v?.trim() ? undefined : "paste the token") }),
    )
  ).trim();
  const spin = p.spinner();
  spin.start("Checking the token");
  let checked;
  try {
    checked = await checkToken(token, domain, {
      accountId: args["account-id"],
      zoneId: args["zone-id"],
      // Required when the deploy token is to give the keys and none came otherwise.
      media: false,
    });
  } catch (e) {
    spin.error("The token won't do");
    throw e;
  }
  if (checked.missing.length > 0) {
    spin.error("The token lacks permissions");
    throw new Error(
      "add these to the token (dash.cloudflare.com/profile/api-tokens → Edit), then run init again:\n" +
        checked.missing.map((m) => `  ${m}`).join("\n"),
    );
  }
  spin.stop(`The token reaches the account and ${domain}`);

  let r2 = { id: "", secret: "" };
  if (!wantsMedia) {
    // --no-media: files off.
  } else if (r2Flag) {
    r2 = isTokenValue(r2Flag)
      ? await r2KeysOf(r2Flag, checked.accountId)
      : { id: r2Flag, secret: (args["r2-secret-access-key"] ?? "").trim() };
    const wrong = checkR2Keys(r2.id, r2.secret);
    if (wrong) throw new Error(`the R2 keys: ${wrong}`);
  } else if (keptR2) {
    r2 = { id: keptR2.get("R2_ACCESS_KEY_ID")!, secret: keptR2.get("R2_SECRET_ACCESS_KEY") ?? "" };
  } else if (checked.r2) {
    r2 = await r2KeysOf(token, checked.accountId);
  } else {
    p.log.warn("The token can't use R2, so uploads are off: add R2 (edit) to it, then run init --force.");
  }
  const media = Boolean(r2.id);

  const email = (
    await ask(args["owner-email"], undefined, () =>
      p.text({ message: "The owner's email (the first staff user)", validate: checkEmail }),
    )
  ).trim();
  if (checkEmail(email)) throw new Error(`--owner-email: ${email} isn't an email address`);
  const storeName = await ask(args["store-name"], "My store", () =>
    p.text({ message: "The store's name", initialValue: "My store" }),
  );

  // Written again: keep what Cloudflare already holds (the resources' IDs).
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
      accountId: checked.accountId,
      zoneId: checked.zoneId,
      ...(args.region ? { region: args.region } : {}),
      // A namespace_id is a number unique per account: a random block per deployment.
      rateLimitBase: earlier?.rateLimitBase ?? (1 + randomInt(90_000)) * BLOCK,
      ...(earlier?.hyperdriveId ? { hyperdriveId: earlier.hyperdriveId } : {}),
      ...(earlier?.kvId ? { kvId: earlier.kvId } : {}),
      ...(earlier?.cacheKvId ? { cacheKvId: earlier.cacheKvId } : {}),
    },
  };
  const env = newCloudflareEnv({ email, storeName }, databaseUrl, {
    token,
    r2,
    storefront: Boolean(domains.storefront),
  });
  // The database URL, token and R2 keys given now win; the secret key, the owner and
  // the storefront's keys stay.
  const given = new Set(["DATABASE_URL", "CLOUDFLARE_API_TOKEN", "R2_ACCESS_KEY_ID", "R2_SECRET_ACCESS_KEY"]);
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
  await writeFile(join(dir, ".gitignore"), `# Secrets: never commit these.\n${ENV_FILE}\n.secrets*.json\n.cors.json\n`);

  p.note(
    [
      `API        ${url(d, domains.api)}`,
      ...(domains.admin ? [`Admin      ${url(d, domains.admin)}`] : []),
      ...(domains.storefront ? [`Storefront ${url(d, domains.storefront)}`] : []),
      `Files      ${media ? url(d, domains.media) : "off (no R2 keys)"}`,
      `Database   ${new URL(databaseUrl).host}`,
    ].join("\n"),
    `Written to ${dir}`,
  );
  p.log.step("hypha deploy needs Rust with `rustup target add wasm32-unknown-unknown` and `cargo install worker-build`.");
  p.outro(`Next: hypha deploy${args.dir === "hypha-deploy" ? "" : ` --dir ${args.dir}`}`);
}

/** Opens `link` in the browser, if this machine has a way; the link is printed anyway. */
function openInBrowser(link: string): void {
  // Not `cmd /c start`: cmd would split the link at its `&`s.
  const [program, ...args] =
    process.platform === "darwin"
      ? ["open"]
      : process.platform === "win32"
        ? ["rundll32", "url.dll,FileProtocolHandler"]
        : ["xdg-open"];
  run(program, [...args, link], { capture: true }).catch(() => {});
}
