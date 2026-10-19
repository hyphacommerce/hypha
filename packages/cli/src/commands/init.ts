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
  type EnvFile,
  loadDeployment,
  loadEnv,
  saveDeployment,
  saveEnv,
  STATE_FILE,
  type Target,
  url,
} from "../deployment.ts";
import {
  type AppEntry,
  appUrl,
  ENCRYPTION_KEY_VAR,
  entryFor,
  FIRST_PARTY,
  remoteHead,
} from "../apps.ts";
import { checkR2Keys, isTokenValue, r2KeysOf } from "../cloudflare-token.ts";
import { BLOCK, checkDatabaseUrl } from "../cloudflare.ts";
import {
  caddyfile,
  composeFile,
  configSchema,
  configToml,
  type EmailSettings,
  newCloudflareEnv,
  newEnv,
  seaweedConfig,
  setEmailSecret,
  type SmtpTls,
} from "../templates.ts";
import { ensureEncryptionKeys } from "./apps.ts";
import { ask, dirArg, guarded, setUnattended } from "./shared.ts";
import { obtainToken } from "./token.ts";

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
  "HYPHA_RESEND_API_KEY",
  "HYPHA_SMTP_PASSWORD",
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
  meta: {
    name: "init",
    description: "Write a deployment: where Hypha runs, its domains, its secrets",
  },
  args: {
    dir: dirArg,
    target: {
      type: "string",
      description:
        "vps (a server over SSH), cloudflare (the API on Workers) or local (this machine, to try it)",
    },
    ssh: { type: "string", description: "The server's SSH destination, e.g. root@203.0.113.7" },
    domain: {
      type: "string",
      description: "Your domain: api., admin. and media. go under it, the shop on it",
    },
    "api-domain": { type: "string", description: "The API's domain (default api.<domain>)" },
    "admin-domain": { type: "string", description: "The admin's domain (default admin.<domain>)" },
    "media-domain": {
      type: "string",
      description: "Uploaded files' domain (default media.<domain>)",
    },
    "storefront-domain": { type: "string", description: "The shop's domain (default <domain>)" },
    admin: {
      type: "boolean",
      default: true,
      description: "Include the admin (--no-admin to host your own)",
    },
    storefront: {
      type: "boolean",
      default: true,
      description: "Include the storefront (--no-storefront for your own)",
    },
    "owner-email": { type: "string", description: "The first staff user, the store's owner" },
    "store-name": { type: "string", description: "The store's name" },
    "acme-email": {
      type: "string",
      description: "For Let's Encrypt's expiry notices (default the owner's; none for none)",
    },
    "build-on": {
      type: "string",
      description: "server (default) or here: where the images are built",
    },
    "remote-dir": {
      type: "string",
      default: "/opt/hypha",
      description: "The deployment's folder on the server",
    },
    "database-url": {
      type: "string",
      description: "cloudflare: the Postgres database's direct URL (any provider; not a pooler's)",
    },
    name: {
      type: "string",
      description: "cloudflare: its resources' prefix (default hypha: hypha-api, hypha-media, …)",
    },
    "account-id": {
      type: "string",
      description: "cloudflare: the account, when wrangler reaches more than one",
    },
    "api-token": {
      type: "string",
      description:
        "cloudflare: the API token to deploy with (else CLOUDFLARE_API_TOKEN, else init links to a filled-in form)",
    },
    "zone-id": {
      type: "string",
      description: "cloudflare: the domain's zone (found from the token otherwise)",
    },
    region: {
      type: "string",
      description:
        "cloudflare: a placement region (aws:eu-central-1) instead of measuring the database's host",
    },
    "r2-access-key-id": {
      type: "string",
      description:
        "cloudflare: R2's Access Key ID, or an R2 token's value (default: derived from the API token)",
    },
    "r2-secret-access-key": {
      type: "string",
      description: "cloudflare: R2's Secret Access Key, with an Access Key ID",
    },
    media: {
      type: "boolean",
      default: true,
      description: "cloudflare: files on R2 (--no-media for none)",
    },
    "email-sender": {
      type: "string",
      description:
        "How emails go out: log (written to the API's log, the default), resend, or smtp (not on cloudflare)",
    },
    "email-from": {
      type: "string",
      description: "The address emails come from, on a domain your provider verified",
    },
    "resend-api-key": { type: "string", description: "resend: the API key (re_…)" },
    "smtp-host": { type: "string", description: "smtp: the server's host name" },
    "smtp-port": { type: "string", description: "smtp: its port (default 587)" },
    "smtp-tls": {
      type: "string",
      description: "smtp: starttls (default), tls (port 465) or none",
    },
    "smtp-username": { type: "string", description: "smtp: the login, if it needs one" },
    "smtp-password": { type: "string", description: "smtp: the login's password" },
    apps: {
      type: "string",
      description:
        "Apps deployed alongside, comma-separated: stripe, meilisearch (none for none; more with `hypha apps add`)",
    },
    yes: { type: "boolean", alias: "y", description: "Don't ask: defaults for what isn't given" },
    force: {
      type: "boolean",
      description: "Write an existing deployment again (keeps its secrets, owner and keys)",
    },
  },
  run: ({ args }) =>
    guarded(async () => {
      const dir = resolve(args.dir);
      setUnattended(Boolean(args.yes));
      p.intro("hypha init");

      if (!args.force && (await exists(join(dir, STATE_FILE)))) {
        p.cancel(
          `${dir} already holds a deployment: \`hypha deploy\` it, or --force to write it again (keeps its secrets)`,
        );
        process.exit(1);
      }

      const target = await ask(args.target, "vps", () =>
        p.select<Target>({
          message: "Where should Hypha run?",
          options: [
            {
              value: "vps",
              label: "A server",
              hint: "a VPS you reach over SSH (Hetzner, DigitalOcean, …)",
            },
            {
              value: "cloudflare",
              label: "Cloudflare",
              hint: "the API on Workers, with your Postgres (Neon, Supabase, …)",
            },
            { value: "local", label: "This machine", hint: "to try the production setup locally" },
          ],
        }),
      );
      if (target !== "vps" && target !== "local" && target !== "cloudflare") {
        throw new Error(`--target is vps, cloudflare or local, not ${target}`);
      }
      if (target === "cloudflare") return initCloudflare(dir, args);

      let ssh: string | undefined;
      let domains: Deployment["domains"];
      let apex = "hypha.localhost";
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
        apex = domain;
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
          ...(args.admin
            ? { admin: await sub(args["admin-domain"], "The admin's address", `admin.${domain}`) }
            : {}),
          ...(args.storefront
            ? { storefront: await sub(args["storefront-domain"], "The shop's address", domain) }
            : {}),
        };
        const buildAnswer = await ask(args["build-on"], "server", () =>
          p.select<BuildOn>({
            message: "Where should the images be built?",
            options: [
              {
                value: "server",
                label: "On the server",
                hint: "nothing to copy; compiling needs ~4 GB of memory",
              },
              { value: "here", label: "Here", hint: "then copied over; for small servers" },
            ],
          }),
        );
        if (buildAnswer !== "server" && buildAnswer !== "here")
          throw new Error(`--build-on is server or here, not ${buildAnswer}`);
        buildOn = buildAnswer;
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
      const emailSettings = await askEmail(args, target, domains.storefront ?? domains.api);

      const acme = args["acme-email"] === "none" ? undefined : args["acme-email"] || email;
      const earlierState = args.force
        ? await loadDeployment(dir).catch(() => undefined)
        : undefined;
      const d: Deployment = {
        version: 1,
        target,
        ...(ssh ? { ssh: ssh.trim() } : {}),
        remoteDir: args["remote-dir"],
        buildOn,
        scheme: target === "vps" ? "https" : "http",
        domains,
        domain: apex,
        ...(acme ? { acmeEmail: acme } : {}),
      };
      const chosenApps = await askApps(args, d, earlierState?.apps);
      if (chosenApps) d.apps = chosenApps;
      const env = setEmailSecret(newEnv(d, { email, storeName }), emailSettings);
      // Written again (new domains, say): the server's data was made with these, so they
      // stay, and so do the owner and the storefront's keys.
      const earlier = args.force ? await loadEnv(dir).catch(() => undefined) : undefined;
      if (earlier)
        for (const key of KEPT) {
          const value = earlier.get(key);
          if (value !== undefined && value !== "" && env.get(key) !== undefined)
            env.set(key, value);
        }
      keepAppKeys(earlier, env);
      ensureEncryptionKeys(d, env);

      await mkdir(join(dir, "seaweedfs"), { recursive: true, mode: 0o700 });
      await chmod(dir, 0o700);
      await saveDeployment(dir, d);
      await saveEnv(dir, env);
      await writeFile(join(dir, "compose.yaml"), await composeFile());
      await writeFile(join(dir, "Caddyfile"), caddyfile(d, acme));
      await writeFile(join(dir, "hypha.config.toml"), configToml(target, emailSettings));
      await writeFile(join(dir, "hypha.config.schema.json"), await configSchema());
      await writeFile(
        join(dir, "seaweedfs", "s3.json"),
        seaweedConfig(env.get("S3_ACCESS_KEY")!, env.get("S3_SECRET_KEY")!),
      );
      // SeaweedFS reads it as its own user; the folder (700) is what keeps it private.
      await chmod(join(dir, "seaweedfs", "s3.json"), 0o644);
      await writeFile(
        join(dir, ".gitignore"),
        `# Secrets: never commit these.\n${ENV_FILE}\nseaweedfs/\n# The co-located apps' clones (hypha deploy makes them again).\n.build/\n`,
      );

      const lines = [
        `API        ${url(d, domains.api)}`,
        ...(domains.admin ? [`Admin      ${url(d, domains.admin)}`] : []),
        ...(domains.storefront ? [`Storefront ${url(d, domains.storefront)}`] : []),
        `Files      ${url(d, domains.media)}`,
        `Emails     ${emailLine(emailSettings)}`,
        ...appLines(d),
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
  apps?: string;
} & EmailArgs;

/**
 * `--target cloudflare` (design note 0026, §5): the API, the admin and the storefront as
 * Workers on your account, the API over Hyperdrive to the Postgres you give (any
 * provider), all deployed with one API token.
 */
async function initCloudflare(dir: string, args: CloudflareArgs): Promise<void> {
  const databaseUrl = (
    await ask(args["database-url"], undefined, () =>
      p.text({
        message:
          "The Postgres database's direct URL (Neon, Supabase, RDS, your own; not a pooler's)",
        placeholder: "postgres://user:password@host:5432/hypha?sslmode=require",
        validate: checkDatabaseUrl,
      }),
    )
  ).trim();
  const problem = checkDatabaseUrl(databaseUrl);
  if (problem) throw new Error(`--database-url: ${problem}`);

  const domain = (
    await ask(args.domain, undefined, () =>
      p.text({
        message: "Your domain, on Cloudflare",
        placeholder: "example.com",
        validate: checkDomain,
      }),
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
  // Each becomes a Worker's custom domain: none may have a DNS record already.
  const domains: Deployment["domains"] = {
    api: await sub(args["api-domain"], "The API's address", `api.${domain}`),
    media: await sub(args["media-domain"], "Uploaded files' address", `media.${domain}`),
    ...(args.admin
      ? { admin: await sub(args["admin-domain"], "The admin's address", `admin.${domain}`) }
      : {}),
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
  if (!NAME.test(name))
    throw new Error(`--name: ${name} should be lowercase letters, digits and dashes`);
  // Files: R2 keys by flag (an R2 token's value is turned into them), else the ones
  // `--force` keeps, else derived from the deploy token if it may use R2.
  const r2Flag = (args["r2-access-key-id"] ?? "").trim();
  const earlierEnv = args.force ? await loadEnv(dir).catch(() => undefined) : undefined;
  const earlierState = args.force ? await loadDeployment(dir).catch(() => undefined) : undefined;
  const keptR2 = earlierEnv?.get("R2_ACCESS_KEY_ID") ? earlierEnv : undefined;
  const wantsMedia = args.media !== false;

  // The token everything deploys with (cloudflare-token.ts).
  const { token, checked } = await obtainToken({
    name,
    domain,
    given: args["api-token"],
    known: earlierEnv?.get("CLOUDFLARE_API_TOKEN") || process.env.CLOUDFLARE_API_TOKEN || undefined,
    accountId: args["account-id"],
    zoneId: args["zone-id"],
    quiet: args.yes,
    apps: Boolean(args.apps && args.apps !== "none") || Boolean(earlierState?.apps?.length),
  });

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
    p.log.warn(
      "The token can't use R2, so uploads are off: add R2 (edit) to it, then run init --force.",
    );
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
  const emailSettings = await askEmail(args, "cloudflare", domains.storefront ?? domains.api);

  // Written again: keep what Cloudflare already holds (the resources' IDs).
  const earlier = earlierState?.cloudflare?.name === name ? earlierState.cloudflare : undefined;
  const d: Deployment = {
    version: 1,
    target: "cloudflare",
    remoteDir: "",
    buildOn: "here",
    scheme: "https",
    domains,
    domain,
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
  // Under another name, an app's Worker and database are other ones: made again.
  const keptApps = earlier
    ? earlierState?.apps
    : earlierState?.apps?.map(({ d1Id: _d1, deployed: _deployed, ...rest }) => rest);
  const chosenApps = await askApps(args, d, keptApps);
  if (chosenApps) d.apps = chosenApps;
  const env = setEmailSecret(
    newCloudflareEnv({ email, storeName }, databaseUrl, {
      token,
      r2,
      storefront: Boolean(domains.storefront),
    }),
    emailSettings,
  );
  // The database URL, token and R2 keys given now win; the secret key, the owner and
  // the storefront's keys stay.
  const given = new Set([
    "DATABASE_URL",
    "CLOUDFLARE_API_TOKEN",
    "R2_ACCESS_KEY_ID",
    "R2_SECRET_ACCESS_KEY",
    "HYPHA_RESEND_API_KEY",
  ]);
  if (earlierEnv)
    for (const key of KEPT) {
      const value = earlierEnv.get(key);
      if (!given.has(key) && value !== undefined && value !== "" && env.get(key) !== undefined)
        env.set(key, value);
    }
  keepAppKeys(earlierEnv, env);
  ensureEncryptionKeys(d, env);

  await mkdir(dir, { recursive: true, mode: 0o700 });
  await chmod(dir, 0o700);
  await saveDeployment(dir, d);
  await saveEnv(dir, env);
  await writeFile(join(dir, "hypha.config.toml"), configToml("cloudflare", emailSettings));
  await writeFile(join(dir, "hypha.config.schema.json"), await configSchema());
  await writeFile(
    join(dir, ".gitignore"),
    `# Secrets: never commit these.\n${ENV_FILE}\n.secrets*.json\n.cors.json\n# The co-located apps' clones (hypha deploy makes them again).\n.build/\n`,
  );

  p.note(
    [
      `API        ${url(d, domains.api)}`,
      ...(domains.admin ? [`Admin      ${url(d, domains.admin)}`] : []),
      ...(domains.storefront ? [`Storefront ${url(d, domains.storefront)}`] : []),
      `Files      ${media ? url(d, domains.media) : "off (no R2 keys)"}`,
      `Emails     ${emailLine(emailSettings)}`,
      `Database   ${new URL(databaseUrl).host}`,
      ...appLines(d),
    ].join("\n"),
    `Written to ${dir}`,
  );
  p.log.step(
    "hypha deploy needs Rust with `rustup target add wasm32-unknown-unknown` and `cargo install worker-build`.",
  );
  p.outro(`Next: hypha deploy${args.dir === "hypha-deploy" ? "" : ` --dir ${args.dir}`}`);
}

/** The flags `askEmail` reads. */
export type EmailArgs = {
  yes?: boolean;
  "email-sender"?: string;
  "email-from"?: string;
  "resend-api-key"?: string;
  "smtp-host"?: string;
  "smtp-port"?: string;
  "smtp-tls"?: string;
  "smtp-username"?: string;
  "smtp-password"?: string;
};

/** Every value answered or flagged, else an error naming the flag. */
function required(value: string, flag: string, why: string): string {
  const trimmed = value.trim();
  if (!trimmed) throw new Error(`--${flag} is required ${why}`);
  return trimmed;
}

/**
 * How the API sends email (design note 0028, §5): the log (nothing is sent; the
 * default), Resend with its API key, or an SMTP server (a server's own, not Workers').
 * `domain` suggests the from address.
 */
export async function askEmail(
  args: EmailArgs,
  target: Target,
  domain: string,
): Promise<EmailSettings> {
  type Sender = EmailSettings["sender"];
  const sender = await ask(args["email-sender"], "log", () =>
    p.select<Sender>({
      message: "How should emails (order confirmations, invites) go out?",
      options: [
        { value: "resend", label: "Resend", hint: "an API key from resend.com" },
        ...(target === "cloudflare"
          ? []
          : [{ value: "smtp" as const, label: "SMTP", hint: "any provider's server" }]),
        { value: "log", label: "Not yet", hint: "written to the API's log; set it up later" },
      ],
    }),
  );
  if (sender !== "log" && sender !== "resend" && sender !== "smtp") {
    throw new Error(`--email-sender is log, resend or smtp, not ${sender}`);
  }
  if (sender === "smtp" && target === "cloudflare") {
    throw new Error("--email-sender smtp isn't available on Cloudflare Workers: use resend");
  }
  if (sender === "log") return { sender };
  const why = `with --email-sender ${sender}`;
  const from = required(
    await ask(args["email-from"], "", () =>
      p.text({
        message: "The address emails come from (on a domain your provider verified)",
        placeholder: `shop@mail.${domain}`,
        validate: checkEmail,
      }),
    ),
    "email-from",
    why,
  );
  if (checkEmail(from)) throw new Error(`--email-from: ${from} isn't an email address`);
  if (sender === "resend") {
    const apiKey = required(
      await ask(args["resend-api-key"], "", () =>
        p.password({
          message: "Resend's API key (resend.com/api-keys, with sending access)",
          validate: (v) => (v?.trim().startsWith("re_") ? undefined : "a key starting re_"),
        }),
      ),
      "resend-api-key",
      why,
    );
    return { sender, from, apiKey };
  }
  const host = required(
    await ask(args["smtp-host"], "", () =>
      p.text({
        message: "The SMTP server",
        placeholder: "smtp.example.com",
        validate: (v) => (v?.trim() ? undefined : "a host name"),
      }),
    ),
    "smtp-host",
    why,
  );
  const tlsAnswer = await ask(args["smtp-tls"], "starttls", () =>
    p.select<SmtpTls>({
      message: "How is the connection encrypted?",
      options: [
        { value: "starttls", label: "STARTTLS", hint: "usually port 587" },
        { value: "tls", label: "TLS", hint: "usually port 465" },
        { value: "none", label: "None", hint: "a server on the same network only" },
      ],
    }),
  );
  if (tlsAnswer !== "starttls" && tlsAnswer !== "tls" && tlsAnswer !== "none") {
    throw new Error(`--smtp-tls is starttls, tls or none, not ${tlsAnswer}`);
  }
  const tls: SmtpTls = tlsAnswer;
  const defaultPort = tls === "tls" ? "465" : "587";
  const portAnswer = await ask(args["smtp-port"], defaultPort, () =>
    p.text({ message: "Its port", initialValue: defaultPort }),
  );
  const port = Number(portAnswer);
  if (!Number.isInteger(port) || port < 1 || port > 65_535) {
    throw new Error(`--smtp-port: ${portAnswer} isn't a port (1 to 65535)`);
  }
  const username = (
    await ask(args["smtp-username"], "", () =>
      p.text({ message: "The login (empty for none)", placeholder: "apikey" }),
    )
  ).trim();
  if (!username) return { sender, from, host, port, tls };
  const password = required(
    await ask(args["smtp-password"], "", () =>
      p.password({
        message: "Its password",
        validate: (v) => (v ? undefined : "the login's password"),
      }),
    ),
    "smtp-password",
    "with --smtp-username",
  );
  return { sender, from, host, port, tls, username, password };
}

function emailLine(email: EmailSettings): string {
  switch (email.sender) {
    case "log":
      return "logged, not sent (set [email] in hypha.config.toml)";
    case "resend":
      return `Resend, from ${email.from}`;
    case "smtp":
      return `SMTP ${email.host}:${email.port}, from ${email.from}`;
  }
}

/** The co-located apps in the summary. */
function appLines(d: Deployment): string[] {
  return (d.apps ?? []).map((a) => `${a.name.padEnd(10)} ${appUrl(d, a)}`);
}

/** Keeps every app's encryption key from the earlier `.env` (`--force`): without it the
 * app loses what it sealed. */
function keepAppKeys(earlier: EnvFile | undefined, env: EnvFile): void {
  if (!earlier) return;
  for (const key of earlier.keys().filter((k) => ENCRYPTION_KEY_VAR.test(k))) {
    const value = earlier.get(key);
    if (value && !env.get(key)) env.set(key, value);
  }
}

/**
 * The first-party apps to deploy alongside (design note 0029, §9): `--apps`, else asked
 * (none by default), each pinned to its repository's head and on `<name>.<domain>`
 * unless another domain is given. With `--force`, the earlier list stays unless `--apps`
 * replaces it (an app kept keeps its commit, domain and resources). `undefined`: none.
 */
async function askApps(
  args: { apps?: string; yes?: boolean },
  d: Deployment,
  earlier: AppEntry[] | undefined,
): Promise<AppEntry[] | undefined> {
  if (args.apps === undefined && earlier) return earlier;
  let names: string[];
  if (args.apps !== undefined) {
    names =
      args.apps === "none"
        ? []
        : args.apps
            .split(",")
            .map((n) => n.trim())
            .filter(Boolean);
  } else if (args.yes || !process.stdin.isTTY) {
    names = [];
  } else {
    const answer = await p.multiselect({
      message: "Deploy apps alongside? (each installed afterwards on its consent page)",
      options: Object.keys(FIRST_PARTY).map((name) => ({
        value: name,
        label: name,
        hint: name === "stripe" ? "card payments" : "search through Meilisearch",
      })),
      required: false,
    });
    if (p.isCancel(answer)) {
      p.cancel("Cancelled.");
      process.exit(1);
    }
    names = answer;
  }
  if (names.length === 0) return undefined;
  const out: AppEntry[] = [];
  for (const name of names) {
    if (!FIRST_PARTY[name])
      throw new Error(
        `--apps: ${name} isn't a first-party app (${Object.keys(FIRST_PARTY).join(", ")}); add others with \`hypha apps add <git url>\``,
      );
    const kept = earlier?.find((a) => a.name === name);
    if (kept) {
      out.push(kept);
      continue;
    }
    const entry = entryFor(name);
    const fallback = `${name}.${d.domain}`;
    const domain =
      d.target === "local"
        ? fallback
        : (
            await ask(undefined, fallback, () =>
              p.text({
                message: `The ${name} app's address`,
                initialValue: fallback,
                validate: checkDomain,
              }),
            )
          ).toLowerCase();
    if (checkDomain(domain)) throw new Error(`the ${name} app's address: ${domain} isn't a domain`);
    if (domain !== fallback) entry.domain = domain;
    try {
      entry.rev = await remoteHead(entry.git);
    } catch (e) {
      // Pinned by the first deploy instead (its head then).
      p.log.warn(
        `Couldn't read ${entry.git}'s head (${e instanceof Error ? e.message.split("\n")[0] : String(e)}): \`hypha deploy\` pins it`,
      );
    }
    out.push(entry);
  }
  return out;
}
