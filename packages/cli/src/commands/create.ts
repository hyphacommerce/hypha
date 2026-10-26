// `hypha create <storefront|app> <dir>`: a project of your own from one of Hypha's starters.

import * as p from "@clack/prompts";
import { defineCommand } from "citty";

import { STARTERS, create, isStarter } from "../create.ts";
import { VERSION } from "../version.ts";
import { ask, guarded, setUnattended } from "./shared.ts";

export const createCommand = defineCommand({
  meta: {
    name: "create",
    description: "A project of your own from a starter: the storefront, or an app",
  },
  args: {
    starter: {
      type: "positional",
      required: false,
      description: `One of ${Object.keys(STARTERS).join(", ")}`,
    },
    dir: { type: "positional", required: false, description: "The new project's folder" },
    ref: {
      type: "string",
      description: `A tag or branch of hyphacommerce/hypha to take the starter from (default v${VERSION}, this CLI's release)`,
    },
    yes: { type: "boolean", default: false, description: "Take defaults instead of asking" },
  },
  run: ({ args }) =>
    guarded(async () => {
      setUnattended(args.yes);
      p.intro(`hypha create (v${VERSION})`);
      const starter = await ask(args.starter, undefined, () =>
        p.select({
          message: "What are you starting?",
          options: Object.entries(STARTERS).map(([value, s]) => ({
            value,
            label: value,
            hint: s.title,
          })),
        }),
      );
      if (!isStarter(starter))
        throw new Error(
          `no starter ${JSON.stringify(starter)}: one of ${Object.keys(STARTERS).join(", ")}`,
        );
      const dir = await ask(args.dir, `my-${starter}`, () =>
        p.text({
          message: "Its folder",
          placeholder: `my-${starter}`,
          defaultValue: `my-${starter}`,
        }),
      );
      const made = await create(starter, dir, { ref: args.ref });
      p.log.success(
        `${made.name} in ${made.dir}, from ${made.source}; Hypha's packages at ${VERSION}`,
      );
      p.note([`cd ${dir}`, ...STARTERS[starter].next].join("\n"), "Next");
      p.outro(`${STARTERS[starter].title}.`);
    }),
});
