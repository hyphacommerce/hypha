import assert from "node:assert/strict";
import { test } from "node:test";

import { EnvFile } from "./deployment.ts";
import { IMAGE_VARS, publishedImage, setImageRefs } from "./images.ts";
import { VERSION } from "./version.ts";

test("the published images are pinned to the CLI's version", () => {
  assert.equal(publishedImage("api"), `ghcr.io/hyphacommerce/hypha-api:${VERSION}`);
  assert.equal(
    publishedImage("storefront", "0.2.0"),
    "ghcr.io/hyphacommerce/hypha-storefront:0.2.0",
  );
});

test("deploying the published images points .env at them; from source clears them", () => {
  const env = EnvFile.parse("POSTGRES_PASSWORD=x\n");
  setImageRefs(env, false, "0.1.0");
  assert.equal(env.get(IMAGE_VARS.api), "ghcr.io/hyphacommerce/hypha-api:0.1.0");
  assert.equal(env.get(IMAGE_VARS.admin), "ghcr.io/hyphacommerce/hypha-admin:0.1.0");
  assert.equal(env.get(IMAGE_VARS.storefront), "ghcr.io/hyphacommerce/hypha-storefront:0.1.0");
  setImageRefs(env, true);
  assert.equal(env.get(IMAGE_VARS.api), undefined);
  assert.equal(env.toString(), "POSTGRES_PASSWORD=x\n");
});
