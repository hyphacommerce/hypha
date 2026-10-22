// The files a deployment runs, made from its settings. `compose.yaml` is the same for
// every server (it reads `.env`), so it's copied as is; the rest depend on the domains
// and the secrets.

import { randomBytes } from "node:crypto";
import { readFile } from "node:fs/promises";
import { join } from "node:path";

import { type AppEntry, type AppProfile, appDomain, CADDY_IMPORT, service } from "./apps.ts";
import { apps, type Deployment, EnvFile, type Target, url } from "./deployment.ts";

const TEMPLATES = join(import.meta.dirname, "..", "templates");
/** The repository the CLI is part of: the images are built from it. */
export const REPO = join(import.meta.dirname, "..", "..", "..");

export function composeFile(): Promise<string> {
  return readFile(join(TEMPLATES, "vps", "compose.yaml"), "utf8");
}

/** The settings file's JSON Schema, for editors. */
export function configSchema(): Promise<string> {
  return readFile(join(REPO, "hypha.config.schema.json"), "utf8");
}

/** Caddy's configuration: one site per domain, to its service. Over `https`, Caddy gets
 * and renews the certificates itself (Let's Encrypt), once the domains point at the
 * server. */
export function caddyfile(d: Deployment, acmeEmail?: string): string {
  const site = (host: string, upstream: string, compress: boolean) =>
    caddySite(d, host, upstream, compress);
  // Let's Encrypt writes to the email before a certificate expires (Caddy renews them
  // first); without one, certificates are issued all the same.
  const global =
    d.scheme === "https"
      ? acmeEmail
        ? ["{", `\temail ${acmeEmail}`, "}"]
        : []
      : ["{", "\tauto_https off", "}"];
  const sites = [
    site(d.domains.api, "api:3000", true),
    // Uploads and public files; SeaweedFS checks upload signatures against this host.
    site(d.domains.media, "s3:8333", false),
    ...(d.domains.admin ? [site(d.domains.admin, "admin:3000", true)] : []),
    ...(d.domains.storefront ? [site(d.domains.storefront, "storefront:3000", true)] : []),
  ];
  return [
    "# Written by `hypha init`; `hypha init --force` writes it again.",
    "",
    ...global,
    "",
    sites.join("\n\n"),
    "",
    "# The co-located apps' sites, written by `hypha deploy`.",
    CADDY_IMPORT,
    "",
  ].join("\n");
}

/** One Caddy site: `host` to `upstream`, compressed or not. */
export function caddySite(d: Deployment, host: string, upstream: string, compress = true): string {
  return [
    `${url(d, host)} {`,
    ...(compress ? ["\tencode zstd gzip"] : []),
    `\treverse_proxy ${upstream}`,
    "}",
  ].join("\n");
}

/** The apps' Caddy sites, each to its container's port. */
export function appsCaddy(
  d: Deployment,
  coLocated: { entry: AppEntry; profile: AppProfile }[],
): string {
  return [
    "# Written by `hypha deploy` from hypha.json's apps.",
    ...coLocated.map(({ entry, profile }) =>
      ["", caddySite(d, appDomain(d, entry), `${service(entry)}:${profile.port}`)].join("\n"),
    ),
    "",
  ].join("\n");
}

/** SeaweedFS's S3 users: the API's key, and anonymous reads of the media bucket (public
 * files are served by plain URL, so static pages can keep them). */
export function seaweedConfig(accessKey: string, secretKey: string): string {
  return `${JSON.stringify(
    {
      identities: [
        {
          name: "hypha",
          credentials: [{ accessKey, secretKey }],
          actions: ["Admin", "Read", "List", "Tagging", "Write"],
        },
        { name: "anonymous", actions: ["Read:hypha-media"] },
      ],
    },
    null,
    2,
  )}\n`;
}

/** How the API sends email: `[email]` in its settings file, the password or key in `.env`.
 * */
export type EmailSettings =
  | { sender: "log" }
  | { sender: "resend"; from: string; apiKey: string }
  | {
      sender: "smtp";
      from: string;
      host: string;
      port: number;
      tls: SmtpTls;
      username?: string;
      password?: string;
    };

export type SmtpTls = "none" | "starttls" | "tls";

/** A TOML string: a JSON string is a valid TOML basic string. */
function str(value: string): string {
  return JSON.stringify(value);
}

/** `[email]` as TOML: the sender and the from address (the secrets stay in `.env`). */
export function emailToml(email: EmailSettings): string[] {
  if (email.sender === "log") {
    return [
      "# Emails are only written to the API's log until a sender is set:",
      '# [email] sender = "resend" (HYPHA_RESEND_API_KEY in .env) or "smtp" ([email.smtp],',
      "# HYPHA_SMTP_PASSWORD in .env), and the from address. `hypha init --force` asks again.",
      "# [email]",
    ];
  }
  const lines = ["[email]", `sender = ${str(email.sender)}`, `from = ${str(email.from)}`];
  if (email.sender === "smtp") {
    lines.push(
      "",
      "[email.smtp]",
      `host = ${str(email.host)}`,
      `port = ${email.port}`,
      `tls = ${str(email.tls)}`,
      ...(email.username ? [`username = ${str(email.username)}`] : []),
    );
  }
  return lines;
}

/** The API's settings file, mostly empty: every key has a default, and the environment
 * in compose.yaml sets the deployment's own. */
export function configToml(
  target: Target = "vps",
  email: EmailSettings = { sender: "log" },
): string {
  const header =
    target === "cloudflare"
      ? [
          "# The API's settings. Every key has a default; the Worker reads",
          "# this file as its HYPHA_CONFIG variable, under the addresses and storage that",
          "# `hypha deploy` sets. After a change: `hypha deploy` (it also resizes the rate",
          "# limiters to [rate_limits]).",
        ]
      : [
          "# The API's settings. Every key has a default; `compose.yaml`",
          "# sets the addresses, storage and proxies through the environment, which wins.",
          "# After a change: `hypha deploy` (or `docker compose up -d api` on the server).",
        ];
  return [
    "#:schema ./hypha.config.schema.json",
    ...header,
    "",
    ...emailToml(email),
    "",
    "# [rate_limits]",
    "# [dispatch]",
    "",
  ].join("\n");
}

/** The email provider's secret in `.env`, which `deploy` passes to the API. */
export function setEmailSecret(env: EnvFile, email: EmailSettings): EnvFile {
  if (email.sender === "resend") {
    env.comment("").comment("Resend's API key, for sending emails.");
    env.set("HYPHA_RESEND_API_KEY", email.apiKey);
  } else if (email.sender === "smtp" && email.password) {
    env.comment("").comment("The SMTP server's password, for sending emails.");
    env.set("HYPHA_SMTP_PASSWORD", email.password);
  }
  return env;
}

export function secret(bytes = 32): string {
  return randomBytes(bytes).toString("hex");
}

/** A password to type once and change: 24 characters, no lookalikes to misread. */
export function password(): string {
  const alphabet = "abcdefghjkmnpqrstuvwxyzABCDEFGHJKMNPQRSTUVWXYZ23456789";
  return Array.from(randomBytes(24), (b) => alphabet[b % alphabet.length]).join("");
}

export type Owner = { email: string; storeName: string };

/** A new deployment's `.env`: the secrets made here, the addresses from the domains. */
export function newEnv(d: Deployment, owner: Owner): EnvFile {
  const env = new EnvFile();
  env
    .comment(
      "Written by `hypha init`. Secrets: keep this file private, and\n" +
        "keep it (HYPHA_SECRET_KEY seals two-factor secrets; losing it locks staff out).",
    )
    .set("POSTGRES_PASSWORD", secret())
    .set("HYPHA_SECRET_KEY", secret())
    .set("S3_ACCESS_KEY", secret(10))
    .set("S3_SECRET_KEY", secret())
    .comment("")
    .comment("The first owner, created on the first start (change the password after logging in).")
    .set("HYPHA_OWNER_EMAIL", owner.email)
    .set("HYPHA_OWNER_PASSWORD", password())
    .set("HYPHA_STORE_NAME", owner.storeName)
    .comment("")
    .comment("Addresses (from hypha.json's domains).")
    .set("API_URL", url(d, d.domains.api))
    .set("MEDIA_URL", url(d, d.domains.media))
    .set("MEDIA_HOST", d.domains.media)
    .set("ADMIN_URL", d.domains.admin ? url(d, d.domains.admin) : "")
    .set("COMPOSE_PROFILES", apps(d).join(","));
  if (d.domains.storefront) {
    env
      .comment("")
      .comment("The storefront's publishable key and webhook secret: `hypha deploy` creates them.")
      .set("STOREFRONT_KEY", "")
      .set("HYPHA_WEBHOOK_SECRET", "");
  }
  return env;
}

/** A Cloudflare deployment's `.env`: kept here, never uploaded as a file. `hypha deploy`
 * passes the token to wrangler and `cf`, the database URL to setup and Hyperdrive, and the
 * rest to the Workers as secrets. */
export function newCloudflareEnv(
  owner: Owner,
  databaseUrl: string,
  options: { token: string; r2: { id: string; secret: string }; storefront: boolean },
): EnvFile {
  const env = new EnvFile()
    .comment(
      "Written by `hypha init --target cloudflare`. Secrets: keep this file\n" +
        "private, and keep it (HYPHA_SECRET_KEY seals two-factor secrets; losing it locks staff out).",
    )
    .set("HYPHA_SECRET_KEY", secret())
    .comment("")
    .comment(
      "The Cloudflare API token everything deploys with (revoke it in the dashboard to stop that).",
    )
    .set("CLOUDFLARE_API_TOKEN", options.token)
    .comment("")
    .comment("The database's direct URL: `hypha deploy` sets it up and points Hyperdrive at it.")
    .set("DATABASE_URL", databaseUrl)
    .comment("")
    .comment(
      "R2's S3 keys for files (derived from an R2-capable token: its ID, the SHA-256 of its value); empty: off.",
    )
    .set("R2_ACCESS_KEY_ID", options.r2.id)
    .set("R2_SECRET_ACCESS_KEY", options.r2.secret)
    .comment("")
    .comment("The first owner, created by the first deploy (change the password after logging in).")
    .set("HYPHA_OWNER_EMAIL", owner.email)
    .set("HYPHA_OWNER_PASSWORD", password())
    .set("HYPHA_STORE_NAME", owner.storeName);
  if (options.storefront) {
    env
      .comment("")
      .comment("The storefront's publishable key and webhook secret: `hypha deploy` creates them.")
      .set("STOREFRONT_KEY", "")
      .set("HYPHA_WEBHOOK_SECRET", "");
  }
  return env;
}
