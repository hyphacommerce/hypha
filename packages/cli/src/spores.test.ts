import assert from "node:assert/strict";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";

import { workerFeatures } from "./cloudflare.ts";
import { chooseSpores } from "./commands/init.ts";
import { describeSpore } from "./commands/spores.ts";
import type { Deployment } from "./deployment.ts";
import { buildArgs, images } from "./images.ts";
import {
  hostCargoToml,
  hostMain,
  needsHostCrate,
  normalizeSpores,
  sporeFeatures,
  writeHostCrate,
} from "./spores.ts";
import { REPO } from "./templates.ts";

const REV = "a".repeat(40);

const reviews = {
  name: "reviews",
  crate: "acme-hypha-reviews",
  spore: "acme_hypha_reviews::Reviews",
  routes: "acme_hypha_reviews::reviews_routes",
  git: "https://github.com/acme/hypha-reviews",
  rev: REV,
};

test("a first-party spore is its name; another is a crate from one source", () => {
  assert.deepEqual(normalizeSpores(["stripe", reviews], "hypha.json"), ["stripe", reviews]);
  assert.throws(() => normalizeSpores(["paypal"], "hypha.json"), /isn't a first-party spore/);
  assert.throws(
    () => normalizeSpores([{ ...reviews, version: "1.0" }], "hypha.json"),
    /one of `git` \(with `rev`\), `version` or `path`/,
  );
  assert.throws(
    () => normalizeSpores([{ ...reviews, rev: "main" }], "hypha.json"),
    /full commit ID/,
  );
  assert.throws(
    () => normalizeSpores([{ ...reviews, spore: "Reviews" }], "hypha.json"),
    /`spore` is a Rust path/,
  );
  assert.throws(() => normalizeSpores("stripe", "hypha.json"), /is a list/);
});

test("spore names are listed once and may not extend one another", () => {
  assert.throws(() => normalizeSpores(["stripe", "stripe"], "x"), /listed twice/);
  assert.throws(
    () => normalizeSpores(["stripe", { ...reviews, name: "stripe_link" }], "x"),
    /extends stripe's/,
  );
  assert.throws(() => normalizeSpores([{ ...reviews, name: "Reviews" }], "x"), /lowercase/);
});

test("first-party spores are the hosts' features; others need a host crate", () => {
  assert.equal(sporeFeatures([]), undefined);
  assert.equal(sporeFeatures(["stripe"]), "spore-stripe");
  assert.equal(needsHostCrate(["stripe"]), false);
  assert.equal(needsHostCrate(["stripe", reviews]), true);
});

test("the API image is built with the spores, as features or the host crate", () => {
  const [plain] = images([], []);
  assert.ok(
    !buildArgs(plain).some((a) => a.startsWith("SPORE_FEATURES") || a.startsWith("HOST_CRATE")),
  );
  const [features] = images([], ["stripe"]);
  assert.ok(buildArgs(features).join(" ").includes("--build-arg SPORE_FEATURES=spore-stripe"));
  const [crate] = images([], ["stripe", reviews], ".hypha-build/x/host");
  const args = buildArgs(crate).join(" ");
  assert.ok(args.includes("--build-arg HOST_CRATE=.hypha-build/x/host"), args);
  assert.ok(!args.includes("SPORE_FEATURES"), args);
});

test("the host crate names the native host and each spore, its own workspace", () => {
  const dir = join(REPO, ".hypha-build", "x", "host");
  const toml = hostCargoToml(dir, ["stripe", reviews]);
  assert.match(toml, /^\[workspace\]$/m);
  assert.match(
    toml,
    /^hypha-host-axum = \{ path = "\.\.\/\.\.\/\.\.\/crates\/hypha-host-axum" \}$/m,
  );
  assert.match(toml, /^hypha-spore-stripe = \{ path = "\.\.\/\.\.\/\.\.\/spores\/stripe" \}$/m);
  assert.ok(
    toml.includes(
      `acme-hypha-reviews = { git = "https://github.com/acme/hypha-reviews", rev = "${REV}" }`,
    ),
    toml,
  );
  const main = hostMain(["stripe", reviews]);
  assert.ok(
    main.includes("spores: [hypha_spore_stripe::Stripe, acme_hypha_reviews::Reviews],"),
    main,
  );
  assert.ok(
    main.includes(
      "routes: [hypha_spore_stripe::stripe_routes, acme_hypha_reviews::reviews_routes],",
    ),
    main,
  );
});

test("writing the host crate seeds its lock from the checkout's, once", async () => {
  const deployment = await mkdtemp(join(tmpdir(), "hypha-spores-"));
  const relative = await writeHostCrate(deployment, ["stripe"]);
  assert.ok(relative.startsWith(".hypha-build/"), relative);
  const lock = await readFile(join(REPO, relative, "Cargo.lock"), "utf8");
  assert.equal(lock, await readFile(join(REPO, "Cargo.lock"), "utf8"));
  const main = await readFile(join(REPO, relative, "src", "main.rs"), "utf8");
  assert.ok(main.includes("hypha_spore_stripe::Stripe"));
  await rm(dirname(join(REPO, relative)), { recursive: true });
});

test("the Worker builds with the first-party spores' features", () => {
  // oxlint-disable-next-line typescript/no-unsafe-type-assertion -- only `spores` is read
  const d = { spores: ["stripe"] } as Deployment;
  assert.equal(workerFeatures(d), " --features spore-stripe");
  // oxlint-disable-next-line typescript/no-unsafe-type-assertion -- only `spores` is read
  assert.equal(workerFeatures({} as Deployment), "");
});

test("init's --spores sets the list, none clears it, absent keeps the earlier", () => {
  assert.deepEqual(chooseSpores({ spores: "stripe" }, undefined), ["stripe"]);
  assert.equal(chooseSpores({ spores: "none" }, ["stripe"]), undefined);
  assert.deepEqual(chooseSpores({}, ["stripe"]), ["stripe"]);
  assert.throws(() => chooseSpores({ spores: "paypal" }, undefined), /first-party/);
});

test("list says where each spore comes from", () => {
  assert.match(describeSpore("stripe"), /this repository's \(spores\/stripe\)/);
  assert.match(
    describeSpore(reviews),
    /acme-hypha-reviews \(https:\/\/github\.com\/acme\/hypha-reviews @ aaaaaaaaaaaa\)/,
  );
});
