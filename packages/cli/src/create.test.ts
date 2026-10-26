import assert from "node:assert/strict";
import { mkdtemp, readFile, readdir, rm, stat, writeFile, mkdir } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import {
  ROOT_ENGINES,
  ROOT_PACKAGE_MANAGER,
  STARTERS,
  adaptManifest,
  archiveOf,
  create,
  projectName,
  tagOf,
} from "./create.ts";
import { REPO } from "./templates.ts";
import { VERSION } from "./version.ts";

test("workspace dependencies become the release's version; the rest stays", () => {
  const out = adaptManifest(
    {
      name: "storefront",
      version: "0.1.0",
      private: true,
      dependencies: { "@hyphacommerce/sdk": "workspace:*", react: "19.3.1" },
      devDependencies: { "@hyphacommerce/app": "workspace:^", typescript: "5.9.3" },
    },
    "my-shop",
    "0.2.0",
    { packageManager: "pnpm@12.6.0", engines: { node: ">=24" } },
  );
  assert.deepEqual(out, {
    name: "my-shop",
    private: true,
    packageManager: "pnpm@12.6.0",
    engines: { node: ">=24" },
    dependencies: { "@hyphacommerce/sdk": "0.2.0", react: "19.3.1" },
    devDependencies: { "@hyphacommerce/app": "0.2.0", typescript: "5.9.3" },
  });
});

test("the project is named after its folder, as npm allows", () => {
  assert.equal(projectName("/tmp/My Shop!"), "my-shop-");
  assert.equal(projectName("./.hidden"), "hidden");
  assert.equal(projectName("acme-storefront"), "acme-storefront");
});

test("a release's archive is the tag's, in GitHub's folder", () => {
  assert.equal(tagOf("0.1.0-beta.3"), "v0.1.0-beta.3");
  assert.deepEqual(archiveOf("0.1.0-beta.3"), {
    url: "https://codeload.github.com/hyphacommerce/hypha/tar.gz/v0.1.0-beta.3",
    root: "hypha-0.1.0-beta.3",
  });
  assert.deepEqual(archiveOf("main"), {
    url: "https://codeload.github.com/hyphacommerce/hypha/tar.gz/main",
    root: "hypha-main",
  });
});

test("what a download assumes of the workspace matches its root package.json", async () => {
  const root = JSON.parse(await readFile(join(REPO, "package.json"), "utf8"));
  assert.equal(root.packageManager, ROOT_PACKAGE_MANAGER);
  assert.deepEqual(root.engines, ROOT_ENGINES);
});

test("every starter's folder is in the repository, with a package.json", async () => {
  for (const starter of Object.values(STARTERS)) {
    const manifest = JSON.parse(await readFile(join(REPO, starter.path, "package.json"), "utf8"));
    assert.equal(manifest.private, true);
  }
});

test("creating from a checkout copies the starter without its builds and adapts it", async () => {
  const work = await mkdtemp(join(tmpdir(), "hypha-create-test-"));
  try {
    const checkout = join(work, "checkout");
    await mkdir(join(checkout, "starters", "app", "node_modules", "x"), { recursive: true });
    await mkdir(join(checkout, "starters", "app", "src"), { recursive: true });
    await writeFile(
      join(checkout, "package.json"),
      JSON.stringify({ packageManager: "pnpm@12.6.0", engines: { node: ">=24" } }),
    );
    await writeFile(
      join(checkout, "starters", "app", "package.json"),
      JSON.stringify({
        name: "app-starter",
        version: "0.1.0",
        private: true,
        dependencies: { "@hyphacommerce/app": "workspace:*" },
      }),
    );
    await writeFile(join(checkout, "starters", "app", "src", "index.ts"), "export {};\n");
    await writeFile(join(checkout, "starters", "app", ".dev.vars"), "SECRET=1\n");
    await writeFile(join(checkout, "starters", "app", "node_modules", "x", "i.js"), "");
    const dir = join(work, "My App");
    const made = await create("app", dir, { from: checkout });
    assert.equal(made.name, "my-app");
    assert.deepEqual((await readdir(made.dir)).toSorted(), [
      ".gitignore",
      "package.json",
      "pnpm-workspace.yaml",
      "src",
    ]);
    const manifest = JSON.parse(await readFile(join(made.dir, "package.json"), "utf8"));
    assert.equal(manifest.name, "my-app");
    assert.equal(manifest.dependencies["@hyphacommerce/app"], VERSION);
    assert.equal(manifest.version, undefined);
    // A second run into the same folder refuses to overwrite it.
    await assert.rejects(create("app", dir, { from: checkout }), /isn't empty/);
    assert.ok(await stat(join(made.dir, "src", "index.ts")));
  } finally {
    await rm(work, { recursive: true, force: true });
  }
});
