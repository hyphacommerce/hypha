// `pnpm dev` / `pnpm prod` with HYPHA_SPORES=stripe: a host crate built with those
// first-party spores (`.hypha-build/dev/host`), each from its checkout beside this one
// (`../spore-<name>`, so a change to it shows at the next start) or else cloned at its
// repository's head. Prints the crate's `Cargo.toml`.

import { stat } from "node:fs/promises";
import { join, resolve } from "node:path";

import { remoteHead } from "./apps.ts";
import { fetchSpores, normalizeSpores, writeHostCrate } from "./spores.ts";
import { REPO } from "./templates.ts";

const names = (process.argv[2] ?? "")
  .split(",")
  .map((s) => s.trim())
  .filter(Boolean);
const spores = normalizeSpores(names, "HYPHA_SPORES");
const local: Record<string, string> = {};
for (const spore of spores) {
  const beside = resolve(REPO, "..", `spore-${spore.name}`);
  const there = await stat(join(beside, "Cargo.toml")).then(
    () => true,
    () => false,
  );
  if (there) {
    local[spore.name] = beside;
    console.error(`spore ${spore.name}: ${beside}`);
  } else if (spore.git) {
    spore.rev = await remoteHead(spore.git);
    Object.assign(local, await fetchSpores("dev", [spore]));
    console.error(`spore ${spore.name}: ${spore.git} @ ${spore.rev.slice(0, 12)}`);
  }
}
const crate = await writeHostCrate("dev", spores, local);
console.log(join(REPO, crate, "Cargo.toml"));
