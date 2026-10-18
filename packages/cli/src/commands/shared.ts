// What the commands share: the deployment's folder, asking only for what no flag
// gave, and connecting to where the stack runs.

import * as p from "@clack/prompts";

import { type Deployment, loadDeployment } from "../deployment.ts";
import { here, type Machine, server } from "../shell.ts";

export const dirArg = {
  type: "string",
  default: "hypha-deploy",
  description: "The deployment's folder",
} as const;

let unattended = false;

/** `--yes`: take defaults instead of asking. */
export function setUnattended(value: boolean): void {
  unattended = value;
}

/**
 * A value from its flag, else `fallback` when not asking (`--yes`, or no terminal),
 * else the prompt's answer. Cancelling a prompt (Ctrl+C) ends the command.
 */
export async function ask(
  flag: string | undefined,
  fallback: string | undefined,
  prompt: () => Promise<string | symbol>,
): Promise<string> {
  if (flag !== undefined && flag !== "") return flag;
  if (fallback !== undefined && (unattended || !process.stdin.isTTY)) {
    return fallback;
  }
  if (!process.stdin.isTTY)
    throw new Error("a value is missing and there's no terminal to ask in: pass its flag");
  const answer = await prompt();
  if (p.isCancel(answer)) {
    p.cancel("Cancelled.");
    process.exit(1);
  }
  return answer as string;
}

export async function open(dir: string): Promise<{ d: Deployment; machine: Machine }> {
  const d = await loadDeployment(dir);
  if (d.target === "vps") {
    if (!d.ssh) throw new Error("hypha.json: a vps deployment needs `ssh`");
    return { d, machine: server(d.ssh, d.remoteDir) };
  }
  return { d, machine: here(dir) };
}

/** Runs `work`, printing a failure as a message instead of a stack trace. */
export async function guarded(work: () => Promise<void>): Promise<void> {
  try {
    await work();
  } catch (e) {
    p.log.error(e instanceof Error ? e.message : String(e));
    p.outro("Stopped.");
    process.exit(1);
  }
}
