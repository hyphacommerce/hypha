// Running programs: here, or on the server over SSH. Everything the CLI does on a
// server goes through `ssh` and `docker`, so it needs nothing installed there but
// Docker, and uses the SSH keys and config the user already has.

import { spawn } from "node:child_process";

export class CommandFailed extends Error {
  readonly command: string;
  readonly code: number | null;
  readonly output: string;

  constructor(command: string, code: number | null, output: string) {
    const tail = output.trim().split("\n").slice(-15).join("\n");
    super(`\`${command}\` failed${code === null ? "" : ` (exit ${code})`}${tail ? `:\n${tail}` : ""}`);
    this.command = command;
    this.code = code;
    this.output = output;
  }
}

export type RunOptions = {
  /** Keep the output (returned) instead of showing it. */
  capture?: boolean;
  /** Written to the program's standard input. */
  input?: string;
  cwd?: string;
  env?: Record<string, string>;
};

/** Runs `program` and resolves with its output when `capture` is set; rejects on a
 * non-zero exit with the output's last lines. */
export function run(program: string, args: string[], options: RunOptions = {}): Promise<string> {
  const { capture = false, input, cwd, env } = options;
  return new Promise((resolve, reject) => {
    const child = spawn(program, args, {
      cwd,
      env: env ? { ...process.env, ...env } : process.env,
      stdio: [input === undefined ? "inherit" : "pipe", capture ? "pipe" : "inherit", capture ? "pipe" : "inherit"],
    });
    let out = "";
    let err = "";
    child.stdout?.on("data", (chunk) => (out += chunk));
    child.stderr?.on("data", (chunk) => (err += chunk));
    if (input !== undefined) child.stdin?.end(input);
    child.on("error", (e) => reject(new CommandFailed(`${program} ${args.join(" ")}`, null, e.message)));
    child.on("close", (code) => {
      if (code === 0) resolve(out);
      else reject(new CommandFailed(`${program} ${args.join(" ")}`, code, `${out}\n${err}`));
    });
  });
}

/** `a | b`: the first program's output is the second's input. */
export function pipe(a: [string, string[]], b: [string, string[]], options: { cwd?: string } = {}): Promise<void> {
  return new Promise((resolve, reject) => {
    const first = spawn(a[0], a[1], { cwd: options.cwd, stdio: ["ignore", "pipe", "inherit"] });
    const second = spawn(b[0], b[1], { stdio: ["pipe", "inherit", "inherit"] });
    first.stdout.pipe(second.stdin);
    let failed = false;
    const fail = (name: string, code: number | null) => {
      if (failed) return;
      failed = true;
      reject(new CommandFailed(name, code, ""));
    };
    first.on("error", () => fail(a[0], null));
    second.on("error", () => fail(b[0], null));
    first.on("close", (code) => code !== 0 && fail(`${a[0]} ${a[1].join(" ")}`, code));
    second.on("close", (code) => (code === 0 ? !failed && resolve() : fail(`${b[0]} ${b[1].join(" ")}`, code)));
  });
}

/** `value` quoted for a POSIX shell. */
export function quote(value: string): string {
  return /^[A-Za-z0-9_./:@=+-]+$/.test(value) ? value : `'${value.replaceAll("'", `'\\''`)}'`;
}

/** Where the stack runs: a server over SSH, or this machine. */
export interface Machine {
  /** For messages: `root@203.0.113.7`, or "this machine". */
  readonly label: string;
  /** The SSH destination, for a server. */
  readonly ssh?: string;
  /** Runs a shell command in the deployment's folder there. */
  sh(command: string, options?: { capture?: boolean; input?: string }): Promise<string>;
  /** Copies `files` (paths relative to `from`) into the deployment's folder there. */
  upload(from: string, files: string[]): Promise<void>;
}

/** A server, over SSH. `BatchMode` makes a missing key an error instead of a prompt. */
export function server(ssh: string, dir: string): Machine {
  const options = ["-o", "BatchMode=yes", "-o", "ConnectTimeout=15"];
  return {
    label: ssh,
    ssh,
    sh(command, { capture, input } = {}) {
      // C.UTF-8 exists everywhere; macOS's `LC_CTYPE=UTF-8`, which ssh passes on, doesn't.
      return run("ssh", [...options, ssh, `export LC_ALL=C.UTF-8; mkdir -p ${quote(dir)} && cd ${quote(dir)} && ${command}`], {
        capture,
        input,
      });
    },
    upload(from, files) {
      return pipe(
        ["tar", ["-C", from, "-czf", "-", ...files]],
        ["ssh", [...options, ssh, `mkdir -p ${quote(dir)} && tar -C ${quote(dir)} -xzf -`]],
      );
    },
  };
}

/** This machine: the deployment's folder is where the files already are. */
export function here(dir: string): Machine {
  return {
    label: "this machine",
    sh(command, { capture, input } = {}) {
      return run("sh", ["-c", command], { cwd: dir, capture, input });
    },
    async upload() {},
  };
}
