// `hypha spores`: the spores compiled into the deployment's API. `add` lists one in
// `hypha.json` (this repository's by name, or a crate), `remove` takes it out, `list`
// shows them. `hypha deploy` builds the API with them; a spore removed keeps its tables
// and its app (shown as missing) until dropped by hand.

import * as p from "@clack/prompts";
import { defineCommand } from "citty";

import { loadDeployment, saveDeployment } from "../deployment.ts";
import {
  FIRST_PARTY_SPORES,
  type SporeEntry,
  isFirstParty,
  normalizeSpores,
  sporeName,
} from "../spores.ts";
import { dirArg, guarded } from "./shared.ts";

/** What `list` shows for a spore. */
export function describeSpore(entry: SporeEntry): string {
  if (isFirstParty(entry)) return `${entry.padEnd(12)} this repository's (spores/${entry})`;
  const source = entry.git
    ? `${entry.git} @ ${entry.rev?.slice(0, 12)}`
    : entry.version
      ? `crates.io ${entry.version}`
      : `path ${entry.path}`;
  return `${entry.name.padEnd(12)} ${entry.crate} (${source})`;
}

export const spores = defineCommand({
  meta: {
    name: "spores",
    description: "Spores compiled into the API (Stripe, or any spore crate)",
  },
  subCommands: {
    list: defineCommand({
      meta: { name: "list", description: "The spores hypha.json lists" },
      args: { dir: dirArg },
      run: ({ args }) =>
        guarded(async () => {
          const d = await loadDeployment(args.dir);
          const listed = d.spores ?? [];
          if (listed.length === 0) {
            p.log.info(
              `No spores: \`hypha spores add ${FIRST_PARTY_SPORES[0]}\` adds this repository's.`,
            );
            return;
          }
          p.log.message(listed.map(describeSpore).join("\n"));
        }),
    }),
    add: defineCommand({
      meta: {
        name: "add",
        description: "List a spore in hypha.json (then `hypha deploy` builds the API with it)",
      },
      args: {
        dir: dirArg,
        spore: {
          type: "positional",
          required: true,
          description: `This repository's (${FIRST_PARTY_SPORES.join(", ")}), or a name with --crate`,
        },
        crate: { type: "string", description: "Its crate (a spore from outside)" },
        type: { type: "string", description: "Its Spore type (crate_name::Type)" },
        routes: {
          type: "string",
          description: "Its route macro (crate_name::name_routes), if it has routes",
        },
        git: { type: "string", description: "Its repository (with --rev)" },
        rev: { type: "string", description: "The commit (40 hex digits)" },
        version: { type: "string", description: "Its crates.io version" },
        path: { type: "string", description: "A local path (relative to the deployment)" },
      },
      run: ({ args }) =>
        guarded(async () => {
          const d = await loadDeployment(args.dir);
          const raw: unknown = args.crate
            ? {
                name: args.spore,
                crate: args.crate,
                spore: args.type,
                ...(args.routes ? { routes: args.routes } : {}),
                ...(args.git ? { git: args.git, rev: args.rev } : {}),
                ...(args.version ? { version: args.version } : {}),
                ...(args.path ? { path: args.path } : {}),
              }
            : args.spore;
          // Checked as hypha.json's list would be, with it in.
          const listed = normalizeSpores([...(d.spores ?? []), raw], "--");
          d.spores = listed;
          await saveDeployment(args.dir, d);
          p.log.success(
            `Listed the ${args.spore} spore: \`hypha deploy\` builds the API with it, and it registers at the API's start.`,
          );
        }),
    }),
    remove: defineCommand({
      meta: {
        name: "remove",
        description:
          "Take a spore out of hypha.json (its tables and app stay, shown as missing, until dropped)",
      },
      args: {
        dir: dirArg,
        spore: { type: "positional", required: true, description: "Its name" },
      },
      run: ({ args }) =>
        guarded(async () => {
          const d = await loadDeployment(args.dir);
          const before = d.spores ?? [];
          const after = before.filter((s) => sporeName(s) !== args.spore);
          if (after.length === before.length)
            throw new Error(`no spore ${args.spore} in hypha.json`);
          d.spores = after;
          await saveDeployment(args.dir, d);
          p.log.success(
            `Took the ${args.spore} spore out: the next \`hypha deploy\` builds the API without it.`,
          );
        }),
    }),
  },
});
