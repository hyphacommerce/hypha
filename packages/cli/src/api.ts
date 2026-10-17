// Talking to a deployed API from here: its health, and the storefront's setup. On this
// machine the domains are `*.localhost`, which not every resolver knows, so requests
// connect to 127.0.0.1 and keep the domain as the host (what Caddy routes by).

import { request as httpRequest } from "node:http";
import { request as httpsRequest } from "node:https";
import type { LookupFunction } from "node:net";

import type { Deployment } from "./deployment.ts";

export class ApiError extends Error {
  readonly status: number;
  readonly code: string | undefined;

  constructor(method: string, path: string, status: number, body: unknown) {
    const parsed = (body ?? {}) as { code?: string; error?: string };
    super(`${method} ${path}: ${status} ${parsed.code ?? ""} ${parsed.error ?? ""}`.trim());
    this.status = status;
    this.code = parsed.code;
  }
}

type Answer = { status: number; body: unknown };

/** Every name to 127.0.0.1: this machine's deployment, whatever its `*.localhost`. */
const loopback: LookupFunction = (_host, options, callback) =>
  options.all ? callback(null, [{ address: "127.0.0.1", family: 4 }]) : callback(null, "127.0.0.1", 4);

function send(d: Deployment, method: string, path: string, body?: unknown, token?: string): Promise<Answer> {
  const target = new URL(path, `${d.scheme}://${d.domains.api}`);
  const request = target.protocol === "https:" ? httpsRequest : httpRequest;
  const payload = body === undefined ? undefined : JSON.stringify(body);
  return new Promise((resolve, reject) => {
    const req = request(
      target,
      {
        method,
        timeout: 15_000,
        headers: {
          accept: "application/json",
          ...(payload ? { "content-type": "application/json" } : {}),
          ...(token ? { authorization: `Bearer ${token}` } : {}),
        },
        ...(d.target === "local" ? { lookup: loopback } : {}),
      },
      (res) => {
        let text = "";
        res.setEncoding("utf8");
        res.on("data", (chunk) => (text += chunk));
        res.on("end", () => {
          let parsed: unknown;
          try {
            parsed = text ? JSON.parse(text) : undefined;
          } catch {
            parsed = text;
          }
          resolve({ status: res.statusCode ?? 0, body: parsed });
        });
      },
    );
    req.on("timeout", () => req.destroy(new Error("timed out")));
    req.on("error", reject);
    req.end(payload);
  });
}

/** Whether `GET /health` answers 200 through Caddy. */
export async function healthy(d: Deployment): Promise<boolean> {
  try {
    return (await send(d, "GET", "/health")).status === 200;
  } catch {
    return false;
  }
}

/** Polls the API's health until it answers or `seconds` pass. The first time, Caddy is
 * also getting the certificates, which can take a minute. */
export async function waitForHealth(d: Deployment, seconds: number): Promise<boolean> {
  const until = Date.now() + seconds * 1000;
  while (Date.now() < until) {
    if (await healthy(d)) return true;
    await new Promise((r) => setTimeout(r, 3000));
  }
  return false;
}

/** A staff session: the API's admin routes, as the owner. */
export class Staff {
  private readonly d: Deployment;
  private readonly token: string;

  private constructor(d: Deployment, token: string) {
    this.d = d;
    this.token = token;
  }

  static async login(d: Deployment, email: string, password: string): Promise<Staff> {
    const { status, body } = await send(d, "POST", "/admin/auth/login", { email, password });
    if (status !== 200) throw new ApiError("POST", "/admin/auth/login", status, body);
    const answer = body as { status: string; session?: { token: string } };
    if (answer.status !== "authenticated" || !answer.session) {
      throw new Error(`logging in needs ${answer.status} (the owner has two-factor on): set the storefront up in the admin`);
    }
    return new Staff(d, answer.session.token);
  }

  async call<T>(method: string, path: string, body?: unknown): Promise<T> {
    const { status, body: answer } = await send(this.d, method, path, body, this.token);
    if (status < 200 || status >= 300) throw new ApiError(method, path, status, answer);
    return answer as T;
  }
}
