import assert from "node:assert/strict";
import { test } from "node:test";

import { askEmail } from "./commands/init.ts";
import { setUnattended } from "./commands/shared.ts";
import { configToml, emailToml, newEnv, setEmailSecret } from "./templates.ts";
import type { Deployment } from "./deployment.ts";

setUnattended(true);

const local: Deployment = {
  version: 1,
  target: "local",
  remoteDir: "/opt/hypha",
  buildOn: "server",
  scheme: "http",
  domains: { api: "api.hypha.localhost", media: "media.hypha.localhost" },
};

test("without a sender, emails are logged and the file says how to send them", async () => {
  const email = await askEmail({}, "vps", "example.com");
  assert.deepEqual(email, { sender: "log" });
  const file = configToml("vps", email);
  assert.match(file, /^# \[email\]$/m);
  assert.doesNotMatch(file, /^\[email\]$/m);
  const env = setEmailSecret(newEnv(local, { email: "o@example.com", storeName: "S" }), email);
  assert.equal(env.get("HYPHA_RESEND_API_KEY"), undefined);
  assert.equal(env.get("HYPHA_SMTP_PASSWORD"), undefined);
});

test("resend writes the sender and from address, and keeps its key in .env", async () => {
  const email = await askEmail(
    {
      "email-sender": "resend",
      "email-from": "shop@mail.example.com",
      "resend-api-key": "re_123",
    },
    "cloudflare",
    "example.com",
  );
  assert.deepEqual(email, {
    sender: "resend",
    from: "shop@mail.example.com",
    apiKey: "re_123",
  });
  assert.deepEqual(emailToml(email), [
    "[email]",
    'sender = "resend"',
    'from = "shop@mail.example.com"',
  ]);
  assert.doesNotMatch(configToml("cloudflare", email), /re_123/);
  const env = setEmailSecret(newEnv(local, { email: "o@example.com", storeName: "S" }), email);
  assert.equal(env.get("HYPHA_RESEND_API_KEY"), "re_123");
});

test("smtp writes its server, and its password to .env", async () => {
  const email = await askEmail(
    {
      "email-sender": "smtp",
      "email-from": "shop@example.com",
      "smtp-host": "smtp.example.com",
      "smtp-tls": "tls",
      "smtp-username": "apikey",
      "smtp-password": "secret",
    },
    "vps",
    "example.com",
  );
  assert.deepEqual(email, {
    sender: "smtp",
    from: "shop@example.com",
    host: "smtp.example.com",
    // TLS from the first byte defaults to 465.
    port: 465,
    tls: "tls",
    username: "apikey",
    password: "secret",
  });
  assert.deepEqual(emailToml(email), [
    "[email]",
    'sender = "smtp"',
    'from = "shop@example.com"',
    "",
    "[email.smtp]",
    'host = "smtp.example.com"',
    "port = 465",
    'tls = "tls"',
    'username = "apikey"',
  ]);
  const env = setEmailSecret(newEnv(local, { email: "o@example.com", storeName: "S" }), email);
  assert.equal(env.get("HYPHA_SMTP_PASSWORD"), "secret");
});

test("smtp without a login has no username and no password", async () => {
  const email = await askEmail(
    { "email-sender": "smtp", "email-from": "shop@example.com", "smtp-host": "mail.internal" },
    "local",
    "hypha.localhost",
  );
  assert.deepEqual(email, {
    sender: "smtp",
    from: "shop@example.com",
    host: "mail.internal",
    port: 587,
    tls: "starttls",
  });
  assert.ok(!emailToml(email).some((line) => line.startsWith("username")));
});

test("what a sender needs is required, and named by its flag", async () => {
  await assert.rejects(
    askEmail({ "email-sender": "resend" }, "vps", "example.com"),
    /--email-from is required with --email-sender resend/,
  );
  await assert.rejects(
    askEmail({ "email-sender": "resend", "email-from": "a@b.co" }, "vps", "example.com"),
    /--resend-api-key is required/,
  );
  await assert.rejects(
    askEmail({ "email-sender": "smtp", "email-from": "a@b.co" }, "vps", "example.com"),
    /--smtp-host is required/,
  );
  await assert.rejects(
    askEmail(
      { "email-sender": "smtp", "email-from": "a@b.co", "smtp-host": "h", "smtp-username": "u" },
      "vps",
      "example.com",
    ),
    /--smtp-password is required with --smtp-username/,
  );
  await assert.rejects(
    askEmail(
      { "email-sender": "smtp", "email-from": "a@b.co", "smtp-host": "h", "smtp-port": "0" },
      "vps",
      "example.com",
    ),
    /--smtp-port: 0 isn't a port/,
  );
  await assert.rejects(
    askEmail({ "email-sender": "resend", "email-from": "nope" }, "vps", "example.com"),
    /--email-from: nope isn't an email address/,
  );
  await assert.rejects(
    askEmail({ "email-sender": "sendgrid" }, "vps", "example.com"),
    /--email-sender is log, resend or smtp, not sendgrid/,
  );
});

test("Workers can't send over SMTP", async () => {
  await assert.rejects(
    askEmail({ "email-sender": "smtp" }, "cloudflare", "example.com"),
    /isn't available on Cloudflare Workers/,
  );
});
