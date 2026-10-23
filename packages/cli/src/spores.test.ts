import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";

import { chooseSpores } from "./commands/init.ts";
import { describeSpore } from "./commands/spores.ts";
import { buildArgs, images } from "./images.ts";
import {
  FIRST_PARTY_SPORES,
  fetchSpores,
  hostCargoToml,
  hostMain,
  normalizeSpores,
  sporeCloneDir,
  workerCargoToml,
  workerDependency,
  writeHostCrate,
  writeWorkerCrate,
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

const stripe = { name: "stripe", ...FIRST_PARTY_SPORES.stripe };

test("a first-party spore's name stands for its repository; another is a crate from one source", () => {
  assert.deepEqual(normalizeSpores(["stripe", reviews], "hypha.json"), [stripe, reviews]);
  // Pinned, as `hypha deploy` saves it: the rest still comes from the repository's entry.
  assert.deepEqual(normalizeSpores([{ name: "stripe", rev: REV }], "hypha.json"), [
    { ...stripe, rev: REV },
  ]);
  assert.throws(() => normalizeSpores(["paypal"], "hypha.json"), /isn't a first-party spore/);
  assert.throws(
    () => normalizeSpores([{ ...reviews, version: "1.0" }], "hypha.json"),
    /one of `git`, `version` or `path`/,
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

test("the API image is the stock host, or the deployment's host crate", () => {
  const [plain] = images([]);
  assert.ok(!buildArgs(plain).some((a) => a.startsWith("HOST_CRATE")));
  const [crate] = images([], ".hypha-build/x/host");
  assert.ok(buildArgs(crate).join(" ").includes("--build-arg HOST_CRATE=.hypha-build/x/host"));
});

test("the host crate builds git spores from their clones, with Hypha patched to the checkout", () => {
  const dir = join(REPO, ".hypha-build", "x", "host");
  const local = { stripe: join(REPO, ".hypha-build", "x", "spores", "stripe") };
  const pinned = { ...stripe, rev: REV };
  const toml = hostCargoToml(
    dir,
    [pinned, { ...reviews, git: undefined, version: "1.2" }],
    ".",
    local,
  );
  assert.match(toml, /^\[workspace\]$/m);
  assert.match(
    toml,
    /^hypha-host-axum = \{ path = "\.\.\/\.\.\/\.\.\/crates\/hypha-host-axum" \}$/m,
  );
  assert.match(toml, /^hypha-spore-stripe = \{ path = "\.\.\/spores\/stripe" \}$/m);
  assert.match(toml, /^acme-hypha-reviews = \{ version = "1\.2" \}$/m);
  // Every crate of the checkout, so a spore's git dependency on Hypha is this checkout.
  assert.match(toml, /^\[patch\."https:\/\/github\.com\/hyphacommerce\/hypha"\]$/m);
  assert.match(toml, /^hypha-spore = \{ path = "\.\.\/\.\.\/\.\.\/crates\/hypha-spore" \}$/m);
  assert.throws(() => hostCargoToml(dir, [pinned]), /hasn't been fetched/);
  const main = hostMain([pinned, reviews]);
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

test("git spores are cloned at their commit beside the host crate; an unpinned one is refused", async () => {
  const deployment = await mkdtemp(join(tmpdir(), "hypha-spores-"));
  const calls: string[][] = [];
  const runner = async (program: string, args: string[]) => {
    calls.push([program, ...args]);
    return "";
  };
  const local = await fetchSpores(
    deployment,
    [
      { ...stripe, rev: REV },
      { ...reviews, git: undefined, version: "1.2" },
    ],
    runner,
  );
  const clone = sporeCloneDir(deployment, "stripe");
  assert.deepEqual(local, { stripe: clone });
  assert.deepEqual(calls[0], ["git", "clone", "--quiet", FIRST_PARTY_SPORES.stripe.git, clone]);
  assert.ok(
    calls.some((c) => c.includes("checkout") && c.includes(REV)),
    JSON.stringify(calls),
  );
  await assert.rejects(fetchSpores(deployment, [stripe], runner), /isn't pinned/);
});

const versioned = { ...reviews, git: undefined, rev: undefined, version: "1.2" };

test("a generated crate's lock follows the checkout's only when that changed", async () => {
  const deployment = await mkdtemp(join(tmpdir(), "hypha-spores-"));
  const relative = await writeHostCrate(deployment, [versioned]);
  assert.ok(relative.startsWith(".hypha-build/"), relative);
  const lockPath = join(REPO, relative, "Cargo.lock");
  const checkout = await readFile(join(REPO, "Cargo.lock"), "utf8");
  assert.equal(await readFile(lockPath, "utf8"), checkout);
  const main = await readFile(join(REPO, relative, "src", "main.rs"), "utf8");
  assert.ok(main.includes("acme_hypha_reviews::Reviews"));
  // What Cargo resolved for the spores stays while the checkout's lock is the same…
  await writeFile(lockPath, `${checkout}# resolved\n`);
  await writeHostCrate(deployment, [versioned]);
  assert.ok((await readFile(lockPath, "utf8")).endsWith("# resolved\n"));
  // …and a lock seeded from another checkout lock is replaced.
  await writeFile(join(REPO, relative, "Cargo.lock.seed"), "stale");
  await writeHostCrate(deployment, [versioned]);
  assert.equal(await readFile(lockPath, "utf8"), checkout);
  await rm(dirname(join(REPO, relative)), { recursive: true });
});

test("the Worker crate depends on the very `worker` the host does", async () => {
  const line = await workerDependency();
  const host = await readFile(join(REPO, "crates", "hypha-host-workers", "Cargo.toml"), "utf8");
  assert.ok(host.split("\n").includes(line), line);
  const dir = join(REPO, ".hypha-build", "x", "worker");
  const toml = workerCargoToml(dir, [versioned], line);
  assert.ok(toml.split("\n").includes(line), toml);
  assert.match(
    toml,
    /^hypha-host-workers = \{ path = "\.\.\/\.\.\/\.\.\/crates\/hypha-host-workers", default-features = false \}$/m,
  );
  const deployment = await mkdtemp(join(tmpdir(), "hypha-spores-"));
  const written = await writeWorkerCrate(deployment, [versioned]);
  const lib = await readFile(join(written, "src", "lib.rs"), "utf8");
  assert.ok(lib.includes("hypha_host_workers::worker!"), lib);
  await rm(dirname(written), { recursive: true });
});

test("init's --spores sets the list, none clears it, absent keeps the earlier", () => {
  assert.deepEqual(chooseSpores({ spores: "stripe" }, undefined), [stripe]);
  assert.equal(chooseSpores({ spores: "none" }, [stripe]), undefined);
  assert.deepEqual(chooseSpores({}, [stripe]), [stripe]);
  assert.throws(() => chooseSpores({ spores: "paypal" }, undefined), /first-party/);
});

test("list says where each spore comes from", () => {
  assert.match(describeSpore(stripe), /hypha-spore-stripe \(.*spore-stripe\.git @ unpinned\)/);
  assert.match(
    describeSpore(reviews),
    /acme-hypha-reviews \(https:\/\/github\.com\/acme\/hypha-reviews @ aaaaaaaaaaaa\)/,
  );
});

test("a path spore must be in the checkout's spores/, and is written relative to the host crate", () => {
  const dir = join(REPO, ".hypha-build", "x", "host");
  const local = {
    ...reviews,
    git: undefined,
    rev: undefined,
    path: join(REPO, "spores", "stripe"),
  };
  delete local.git;
  delete local.rev;
  const toml = hostCargoToml(dir, [local]);
  assert.ok(toml.includes('acme-hypha-reviews = { path = "../../../spores/stripe" }'), toml);
  assert.throws(
    () => hostCargoToml(dir, [{ ...local, path: "/elsewhere/reviews" }]),
    /outside .*spores, which the API is built from/,
  );
  assert.throws(
    () => hostCargoToml(dir, [{ ...local, path: join(REPO, "apps", "reviews") }]),
    /outside .*spores/,
  );
});
