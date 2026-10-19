// `hypha apps`: the apps deployed alongside Hypha (design note 0029, §9). `add` lists one
// in `hypha.json` pinned to its repository's head, `update` moves the pin, `remove`
// takes it out, `status` says whether each answers and is installed in the store, and
// `install` opens the install links of those that aren't. `hypha deploy` builds and runs
// them.

import * as p from "@clack/prompts";
import { defineCommand } from "citty";

import { appStatuses, staffFor } from "../app-install.ts";
import {
  type AppEntry,
  appDomain,
  appUrl,
  appWorkerName,
  checkAppName,
  encryptionKeyVar,
  entryFor,
  newEncryptionKey,
  remoteHead,
  service,
} from "../apps.ts";
import {
  type Deployment,
  type EnvFile,
  loadDeployment,
  loadEnv,
  saveDeployment,
  saveEnv,
} from "../deployment.ts";
import { dirArg, guarded } from "./shared.ts";
import { openInBrowser } from "./token.ts";

const DOMAIN = /^(?=.{1,253}$)([a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z][a-z0-9-]{0,62}$/;

/** `rev` as a commit: a full commit ID as given, else a branch or tag asked of the remote. */
export async function resolveRev(git: string, rev: string | undefined): Promise<string> {
  if (rev && /^[0-9a-f]{40}$/.test(rev)) return rev;
  if (rev && /^[0-9a-f]{7,39}$/.test(rev))
    throw new Error(`--rev ${rev}: give the full commit ID (40 hex digits), or a branch or tag`);
  return remoteHead(git, rev ? rev : "HEAD");
}

/** Pins every app listed without a commit (a first-party name alone in `hypha.json`) to
 * its repository's head, and saves it. */
export async function pinUnpinned(dir: string, d: Deployment): Promise<void> {
  let changed = false;
  for (const entry of d.apps ?? []) {
    if (entry.rev) continue;
    entry.rev = await remoteHead(entry.git);
    p.log.step(`Pinned the ${entry.name} app to ${entry.rev.slice(0, 12)} (its head)`);
    changed = true;
  }
  if (changed) await saveDeployment(dir, d);
}

/** Makes each app's `APP_ENCRYPTION_KEY` that `.env` lacks; whether it made one. */
export function ensureEncryptionKeys(d: Deployment, env: EnvFile): boolean {
  let made = false;
  for (const entry of d.apps ?? []) {
    if (env.get(encryptionKeyVar(entry))) continue;
    env
      .comment("")
      .comment(`The ${entry.name} app's APP_ENCRYPTION_KEY: it seals the app's secrets. Keep it.`)
      .set(encryptionKeyVar(entry), newEncryptionKey());
    made = true;
  }
  return made;
}

/**
 * For each co-located app the store hasn't installed: its install link, printed and
 * (in a terminal, unless `quiet`) opened. A staff member approves it on the consent
 * page; nothing installs silently (§4.1).
 */
export async function offerInstalls(
  d: Deployment,
  env: EnvFile | undefined,
  options: { quiet?: boolean; only?: string } = {},
): Promise<void> {
  const apps = (d.apps ?? []).filter((a) => !options.only || a.name === options.only);
  if (apps.length === 0) return;
  const staff = await staffFor(d, env);
  const statuses = await appStatuses({ ...d, apps }, staff);
  const pending = statuses.filter((s) => s.installed !== true);
  for (const done of statuses.filter((s) => s.installed === true))
    p.log.step(`The ${done.entry.name} app is installed (${done.key})`);
  if (pending.length === 0) return;
  p.note(
    pending.map((s) => `${s.entry.name.padEnd(12)} ${s.link}`).join("\n"),
    staff
      ? "Install these apps: open each link and approve it as a staff member"
      : "Install these apps (if the store hasn't): open each link and approve it",
  );
  if (process.stdin.isTTY && !options.quiet) for (const s of pending) openInBrowser(s.link);
}

export const apps = defineCommand({
  meta: {
    name: "apps",
    description: "Apps deployed alongside Hypha (Stripe, Meilisearch, or any app's repository)",
  },
  subCommands: {
    add: defineCommand({
      meta: {
        name: "add",
        description: "List an app in hypha.json, pinned to its repository's head (then deploy)",
      },
      args: {
        dir: dirArg,
        app: {
          type: "positional",
          required: true,
          description: "stripe, meilisearch, or a git URL",
        },
        rev: { type: "string", description: "A commit, branch or tag (default: the head)" },
        name: { type: "string", description: "Its name (default: from the repository's)" },
        domain: { type: "string", description: "Its domain (default <name>.<your domain>)" },
      },
      run: ({ args }) =>
        guarded(async () => {
          const d = await loadDeployment(args.dir);
          p.intro("hypha apps add");
          const entry: AppEntry = entryFor(args.app, args.name);
          if ((d.apps ?? []).some((a) => a.name === entry.name)) {
            throw new Error(
              `the ${entry.name} app is listed already: \`hypha apps update ${entry.name}\` moves its commit`,
            );
          }
          if (args.domain) {
            const domain = args.domain.toLowerCase();
            if (!DOMAIN.test(domain)) throw new Error(`--domain: ${args.domain} isn't a domain`);
            entry.domain = domain;
          }
          entry.rev = await resolveRev(entry.git, args.rev);
          d.apps = [...(d.apps ?? []), entry];
          if (Object.values(d.domains).includes(appDomain(d, entry))) {
            throw new Error(`${appDomain(d, entry)} is taken by the deployment: pass --domain`);
          }
          await saveDeployment(args.dir, d);
          const env = await loadEnv(args.dir).catch(() => undefined);
          if (env && ensureEncryptionKeys(d, env)) await saveEnv(args.dir, env);
          p.note(
            [
              `Repository ${entry.git}`,
              `Commit     ${entry.rev}`,
              `URL        ${appUrl(d, entry)}`,
              d.target === "cloudflare"
                ? `Worker     ${appWorkerName(d, entry)} (with its D1 database)`
                : `Container  ${service(entry)}`,
            ].join("\n"),
            `Added the ${entry.name} app`,
          );
          if (d.target === "vps")
            p.log.step(`Point ${appDomain(d, entry)} at the server before deploying.`);
          if (d.target === "cloudflare")
            p.log.step("The deploy token needs D1 (edit) for the app's database.");
          p.outro("Next: hypha deploy (then approve the app's install)");
        }),
    }),
    update: defineCommand({
      meta: { name: "update", description: "Move an app (or every app) to its head, or --rev" },
      args: {
        dir: dirArg,
        app: { type: "positional", required: false, description: "One app; all without" },
        rev: { type: "string", description: "A commit, branch or tag (with one app)" },
      },
      run: ({ args }) =>
        guarded(async () => {
          const d = await loadDeployment(args.dir);
          p.intro("hypha apps update");
          const listed = d.apps ?? [];
          if (args.rev && !args.app) throw new Error("--rev needs the app's name");
          const chosen = args.app ? listed.filter((a) => a.name === args.app) : listed;
          if (args.app && chosen.length === 0)
            throw new Error(`no ${args.app} app in hypha.json: \`hypha apps add ${args.app}\``);
          if (chosen.length === 0) throw new Error("no apps in hypha.json: `hypha apps add`");
          for (const entry of chosen) {
            const rev = await resolveRev(entry.git, args.rev);
            p.log.step(
              entry.rev === rev
                ? `${entry.name}: at ${rev.slice(0, 12)} already`
                : `${entry.name}: ${entry.rev?.slice(0, 12) ?? "unpinned"} → ${rev.slice(0, 12)}`,
            );
            entry.rev = rev;
          }
          await saveDeployment(args.dir, d);
          p.outro("Next: hypha deploy");
        }),
    }),
    remove: defineCommand({
      meta: { name: "remove", description: "Take an app out of hypha.json" },
      args: {
        dir: dirArg,
        app: { type: "positional", required: true, description: "The app's name" },
      },
      run: ({ args }) =>
        guarded(async () => {
          const wrong = checkAppName(args.app);
          if (wrong) throw new Error(`${args.app}: ${wrong}`);
          const d = await loadDeployment(args.dir);
          p.intro("hypha apps remove");
          const entry = (d.apps ?? []).find((a) => a.name === args.app);
          if (!entry) throw new Error(`no ${args.app} app in hypha.json`);
          d.apps = (d.apps ?? []).filter((a) => a !== entry);
          await saveDeployment(args.dir, d);
          p.log.step(
            d.target === "cloudflare"
              ? `The next deploy unbinds it; its Worker ${appWorkerName(d, entry)} and D1 database stay until you delete them (wrangler delete, wrangler d1 delete).`
              : `The next deploy stops its container; its data volume stays (docker volume rm).`,
          );
          p.log.step(
            "The store keeps its installation: uninstall it in the admin (Settings → Apps).",
          );
          p.outro(`Removed. Keep ${encryptionKeyVar(entry)} in .env if you might bring it back.`);
        }),
    }),
    status: defineCommand({
      meta: {
        name: "status",
        description: "Whether each app answers and is installed in the store",
      },
      args: { dir: dirArg },
      run: ({ args }) =>
        guarded(async () => {
          const d = await loadDeployment(args.dir);
          p.intro("hypha apps status");
          if (!d.apps?.length) {
            p.outro("No apps: `hypha apps add stripe`, say");
            return;
          }
          const env = await loadEnv(args.dir).catch(() => undefined);
          const staff = await staffFor(d, env);
          if (!staff)
            p.log.warn(
              "Can't read the store's apps: set HYPHA_ADMIN_KEY (a secret key with apps:read), or the owner's login in .env",
            );
          const statuses = await appStatuses(d, staff);
          for (const s of statuses) {
            const installed =
              s.installed === undefined ? "?" : s.installed ? "installed" : "not installed";
            p.log.step(
              [
                `${s.entry.name.padEnd(12)} ${s.entry.rev?.slice(0, 12) ?? "unpinned".padEnd(12)}  ${s.url}`,
                `${"".padEnd(12)} ${s.reachable ? "answers" : "doesn't answer"}, ${installed} (${s.key})`,
                ...(s.installed === false ? [`${"".padEnd(12)} install: ${s.link}`] : []),
              ].join("\n"),
            );
          }
          const bad = statuses.some((s) => !s.reachable || s.installed === false);
          p.outro(bad ? "Not all ready" : "Every app answers and is installed");
          if (bad) process.exitCode = 1;
        }),
    }),
    install: defineCommand({
      meta: {
        name: "install",
        description: "Open the install link of each app the store hasn't installed",
      },
      args: {
        dir: dirArg,
        app: { type: "positional", required: false, description: "One app; all without" },
        yes: { type: "boolean", alias: "y", description: "Print the links, don't open them" },
      },
      run: ({ args }) =>
        guarded(async () => {
          const d = await loadDeployment(args.dir);
          p.intro("hypha apps install");
          if (args.app && !(d.apps ?? []).some((a) => a.name === args.app))
            throw new Error(`no ${args.app} app in hypha.json`);
          const env = await loadEnv(args.dir).catch(() => undefined);
          await offerInstalls(d, env, { quiet: Boolean(args.yes), only: args.app });
          p.outro(
            `Then: hypha apps status${args.dir === "hypha-deploy" ? "" : ` --dir ${args.dir}`}`,
          );
        }),
    }),
  },
});
