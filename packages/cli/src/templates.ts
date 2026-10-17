// The files a deployment runs, made from its settings. `compose.yaml` is the same for
// every server (it reads `.env`), so it's copied as is; the rest depend on the domains
// and the secrets.

import { randomBytes } from "node:crypto";
import { readFile } from "node:fs/promises";
import { join } from "node:path";

import { apps, type Deployment, EnvFile, url } from "./deployment.ts";

const TEMPLATES = join(import.meta.dirname, "..", "templates");
/** The repository the CLI is part of: the images are built from it. */
export const REPO = join(import.meta.dirname, "..", "..", "..");

export function composeFile(): Promise<string> {
  return readFile(join(TEMPLATES, "vps", "compose.yaml"), "utf8");
}

/** The settings file's JSON Schema, for editors (design note 0017, CF7). */
export function configSchema(): Promise<string> {
  return readFile(join(REPO, "hypha.config.schema.json"), "utf8");
}

/** Caddy's configuration: one site per domain, to its service. Over `https`, Caddy gets
 * and renews the certificates itself (Let's Encrypt), once the domains point at the
 * server. */
export function caddyfile(d: Deployment): string {
  const site = (host: string, upstream: string, compress: boolean) =>
    [
      `${url(d, host)} {`,
      ...(compress ? ["\tencode zstd gzip"] : []),
      `\treverse_proxy ${upstream}`,
      "}",
    ].join("\n");
  const global =
    d.scheme === "https" ? ["{", "\temail {$ACME_EMAIL}", "}"] : ["{", "\tauto_https off", "}"];
  const sites = [
    site(d.domains.api, "api:3000", true),
    // Uploads and public files; SeaweedFS checks upload signatures against this host.
    site(d.domains.media, "s3:8333", false),
    ...(d.domains.admin ? [site(d.domains.admin, "admin:3000", true)] : []),
    ...(d.domains.storefront ? [site(d.domains.storefront, "storefront:3000", true)] : []),
  ];
  return [
    "# Written by `hypha init` (design note 0025); `hypha init --force` writes it again.",
    "",
    ...global,
    "",
    sites.join("\n\n"),
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

/** The API's settings file, mostly empty: every key has a default, and the environment
 * in compose.yaml sets the deployment's own. */
export function configToml(): string {
  return [
    "#:schema ./hypha.config.schema.json",
    "# The API's settings (design note 0017). Every key has a default; `compose.yaml`",
    "# sets the addresses, storage and proxies through the environment, which wins.",
    "# After a change: `hypha deploy` (or `docker compose up -d api` on the server).",
    "",
    "# [rate_limits]",
    "# [dispatch]",
    "",
  ].join("\n");
}

export function secret(bytes = 32): string {
  return randomBytes(bytes).toString("hex");
}

/** A password to type once and change: 24 characters, no lookalikes to misread. */
export function password(): string {
  const alphabet = "abcdefghjkmnpqrstuvwxyzABCDEFGHJKMNPQRSTUVWXYZ23456789";
  return Array.from(randomBytes(24), (b) => alphabet[b % alphabet.length]).join("");
}

export type Owner = { email: string; storeName: string; acmeEmail?: string };

/** A new deployment's `.env`: the secrets made here, the addresses from the domains. */
export function newEnv(d: Deployment, owner: Owner): EnvFile {
  const env = new EnvFile();
  env
    .comment(
      "Written by `hypha init` (design note 0025). Secrets: keep this file private, and\n" +
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
    .set("MEDIA_URL", url(d, d.domains.media))
    .set("MEDIA_HOST", d.domains.media)
    .set("ADMIN_URL", d.domains.admin ? url(d, d.domains.admin) : "")
    .set("COMPOSE_PROFILES", apps(d).join(","));
  if (d.scheme === "https") env.set("ACME_EMAIL", owner.acmeEmail ?? owner.email);
  if (d.domains.storefront) {
    env
      .comment("")
      .comment("The storefront's publishable key and webhook secret: `hypha deploy` creates them.")
      .set("STOREFRONT_KEY", "")
      .set("HYPHA_WEBHOOK_SECRET", "");
  }
  return env;
}
