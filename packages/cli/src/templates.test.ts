import assert from "node:assert/strict";
import { test } from "node:test";

import { type Deployment, EnvFile } from "./deployment.ts";
import { caddyfile, newEnv, password } from "./templates.ts";

const vps: Deployment = {
  version: 1,
  target: "vps",
  ssh: "root@203.0.113.7",
  remoteDir: "/opt/hypha",
  buildOn: "server",
  scheme: "https",
  domains: {
    api: "api.example.com",
    media: "media.example.com",
    admin: "admin.example.com",
    storefront: "example.com",
  },
};

test("a .env keeps its comments and order when a value changes", () => {
  const env = EnvFile.parse("# secrets\nA=1\n\nB=two\n");
  env.set("A", "3").set("C", "new");
  assert.equal(env.toString(), "# secrets\nA=3\n\nB=two\nC=new\n");
});

test("a value with spaces or quotes is quoted, and reads back the same", () => {
  const env = new EnvFile().set("NAME", 'Lumen & "Loom"');
  assert.equal(env.toString(), 'NAME="Lumen & \\"Loom\\""\n');
  assert.equal(EnvFile.parse(env.toString()).get("NAME"), 'Lumen & "Loom"');
});

test("an empty value stays empty", () => {
  assert.equal(EnvFile.parse("KEY=\n").get("KEY"), "");
});

test("a server's Caddyfile asks for certificates and routes every domain", () => {
  const file = caddyfile(vps, "ops@example.com");
  assert.match(file, /^\{\n\temail ops@example\.com\n\}$/m);
  assert.match(
    file,
    /https:\/\/api\.example\.com \{\n\tencode zstd gzip\n\treverse_proxy api:3000\n\}/,
  );
  assert.match(file, /https:\/\/media\.example\.com \{\n\treverse_proxy s3:8333\n\}/);
  assert.match(file, /https:\/\/admin\.example\.com \{/);
  assert.match(
    file,
    /https:\/\/example\.com \{\n\tencode zstd gzip\n\treverse_proxy storefront:3000/,
  );
});

test("without a storefront, there's no site for it and no profile", () => {
  const d = { ...vps, domains: { ...vps.domains, storefront: undefined } };
  assert.doesNotMatch(caddyfile(d), /storefront:3000/);
  const env = newEnv(d, { email: "owner@example.com", storeName: "Shop" });
  assert.equal(env.get("COMPOSE_PROFILES"), "admin");
  assert.equal(env.get("STOREFRONT_KEY"), undefined);
});

test("this machine's deployment is plain http, without certificates", () => {
  const local: Deployment = {
    ...vps,
    target: "local",
    scheme: "http",
    domains: {
      api: "api.hypha.localhost",
      media: "media.hypha.localhost",
      storefront: "shop.hypha.localhost",
    },
  };
  assert.match(caddyfile(local), /auto_https off/);
  const env = newEnv(local, { email: "owner@example.com", storeName: "Shop" });
  assert.equal(env.get("MEDIA_URL"), "http://media.hypha.localhost");
});

test("without a contact email, a server's Caddyfile has no global options", () => {
  assert.doesNotMatch(caddyfile(vps), /email|auto_https/);
});

test("every secret is new", () => {
  const a = newEnv(vps, { email: "o@example.com", storeName: "S" });
  const b = newEnv(vps, { email: "o@example.com", storeName: "S" });
  for (const key of [
    "POSTGRES_PASSWORD",
    "HYPHA_SECRET_KEY",
    "S3_SECRET_KEY",
    "HYPHA_OWNER_PASSWORD",
  ]) {
    assert.notEqual(a.get(key), b.get(key), key);
  }
  assert.match(a.get("HYPHA_SECRET_KEY")!, /^[0-9a-f]{64}$/);
  assert.equal(password().length, 24);
});
