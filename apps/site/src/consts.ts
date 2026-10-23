/** Links used across the site. */
export const GITHUB = "https://github.com/hyphacommerce/hypha";

/** Where "Sponsor" points. GitHub Sponsors isn't set up for the organisation yet: change
 * this once it is (or to Open Collective). */
export const SPONSOR = "https://github.com/sponsors/hyphacommerce";

/** Where people and sponsors write to. The mailbox has to exist: forward it with
 * Cloudflare Email Routing (free) or a mail provider. */
export const EMAIL = "hello@hyphacommerce.com";

/** The public demo (Mycelium Works). Its login is read-only and locked against changing
 * itself (`auth.locked_accounts` in the demo's hypha.config.toml). */
export const DEMO_STORE = "https://demo.hyphacommerce.com";
export const DEMO_ADMIN = "https://demo-admin.hyphacommerce.com";
export const DEMO_LOGIN = { email: "demo@hyphacommerce.com", password: "hypha-demo" };

export const SITE_TITLE = "Hypha";
export const SITE_DESCRIPTION =
  "Open-source commerce you own: a small Rust core that apps, plugins, databases, hosts and AI agents connect to. Built for merchants, developers and agents, and cheap enough to run a whole store on a five-euro server.";
