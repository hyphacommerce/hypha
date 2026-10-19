// The Cloudflare API token a deployment runs on, asked for by `init`, and by `deploy`
// when `.env` has none: the link to the filled-in form, the token pasted back, and its
// check. A token that lacks a permission can be edited in the dashboard and checked
// again in place, without starting over.

import * as p from "@clack/prompts";

import { type Checked, checkToken, templateUrl } from "../cloudflare-token.ts";
import { run } from "../shell.ts";
import { ask } from "./shared.ts";

export type TokenOptions = {
  /** The deployment's name, for the token's. */
  name: string;
  /** A domain in the zone the token must reach. */
  domain: string;
  /** A token already given (`--api-token`) or known (the earlier `.env`). */
  given?: string;
  known?: string;
  accountId?: string;
  zoneId?: string;
  /** Don't open the browser (`--yes`). */
  quiet?: boolean;
  /** Apps are deployed alongside, so the token needs D1 too. */
  apps?: boolean;
};

export async function obtainToken(o: TokenOptions): Promise<{ token: string; checked: Checked }> {
  if (!o.given && !o.known) {
    const link = templateUrl(`Hypha ${o.name}`);
    p.log.step(
      "Hypha deploys with a Cloudflare API token. Create one with this link (the permissions are\n" +
        `filled in; pick the account and ${o.domain}'s zone), then paste it here:\n${link}`,
    );
    if (process.stdin.isTTY && !o.quiet) openInBrowser(link);
  }
  const token = (
    await ask(o.given, o.known, () =>
      p.password({
        message: "The API token",
        validate: (v) => (v?.trim() ? undefined : "paste the token"),
      }),
    )
  ).trim();
  for (;;) {
    const spin = p.spinner();
    spin.start("Checking the token");
    let checked: Checked;
    try {
      checked = await checkToken(token, o.domain, {
        accountId: o.accountId,
        zoneId: o.zoneId,
        media: false,
        apps: o.apps,
      });
    } catch (e) {
      spin.error("The token won't do");
      throw e;
    }
    if (checked.missing.length === 0) {
      spin.stop(`The token reaches the account and ${o.domain}`);
      return { token, checked };
    }
    spin.error("The token lacks permissions");
    const missing = checked.missing.map((m) => `  ${m}`).join("\n");
    const where = "dash.cloudflare.com/profile/api-tokens → the token's ⋯ → Edit";
    if (!process.stdin.isTTY || o.quiet) {
      throw new Error(`add these to the token (${where}), then run this again:\n${missing}`);
    }
    // Editing a token keeps its value: check the same one again.
    const again = await p.confirm({
      message: `Add these to the token (${where}), then check again:\n${missing}`,
    });
    if (p.isCancel(again) || !again) throw new Error("the token lacks permissions");
  }
}

/** Opens `link` in the browser, if this machine has a way; the link is printed anyway. */
export function openInBrowser(link: string): void {
  // Not `cmd /c start`: cmd would split the link at its `&`s.
  const [program, ...args] =
    process.platform === "darwin"
      ? ["open"]
      : process.platform === "win32"
        ? ["rundll32", "url.dll,FileProtocolHandler"]
        : ["xdg-open"];
  run(program, [...args, link], { capture: true }).catch(() => {});
}
