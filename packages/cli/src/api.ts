// Talking to a deployed API from here: its health, and the storefront's setup. On this
// machine the domains are `*.localhost`, which not every resolver knows, so requests
// connect to 127.0.0.1 and keep the domain as the host (what Caddy routes by). A
// deployed domain is looked up through public resolvers first: a domain just created
// (a Worker's custom domain, new DNS records) that this machine asked for too early
// stays "not found" in its resolver's cache for minutes, though it already answers.

import { lookup as systemLookup } from "node:dns";
import { Resolver } from "node:dns/promises";
import { request as httpRequest } from "node:http";
import { request as httpsRequest } from "node:https";
import { isIP, type LookupFunction } from "node:net";

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

type Address = { address: string; family: 4 | 6 };
type Resolve = (host: string) => Promise<string[]>;

/** A lookup through `resolve4`/`resolve6` (IPv4 first), falling back to `fallback` for
 * IP literals and for names they don't know (a split-horizon or private name). */
export function lookupVia(resolve4: Resolve, resolve6: Resolve, fallback: LookupFunction): LookupFunction {
  return (host, options, callback) => {
    if (isIP(host)) return fallback(host, options, callback);
    const family = options.family === 4 || options.family === "IPv4" ? 4 : options.family === 6 || options.family === "IPv6" ? 6 : 0;
    Promise.all([
      family === 6 ? [] : resolve4(host).catch(() => []),
      family === 4 ? [] : resolve6(host).catch(() => []),
    ]).then(([v4, v6]) => {
      const found: Address[] = [
        ...v4.map((address) => ({ address, family: 4 as const })),
        ...v6.map((address) => ({ address, family: 6 as const })),
      ];
      if (found.length === 0) return fallback(host, options, callback);
      if (options.all) callback(null, found);
      else callback(null, found[0].address, found[0].family);
    });
  };
}

const resolver = new Resolver({ timeout: 3000, tries: 2 });
resolver.setServers(["1.1.1.1", "8.8.8.8"]);

/** Public resolvers first, then this machine's. */
const freshLookup = lookupVia(
  (host) => resolver.resolve4(host),
  (host) => resolver.resolve6(host),
  systemLookup as LookupFunction,
);

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
        lookup: d.target === "local" ? loopback : freshLookup,
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

/** Polls `target` until it answers anything but a server error, or `seconds` pass: a new
 * app's custom domain (its DNS record and certificate) can take a minute or two. */
export async function waitForAnswer(target: string, seconds: number): Promise<boolean> {
  const until = Date.now() + seconds * 1000;
  const parsed = new URL(target);
  const request = parsed.protocol === "https:" ? httpsRequest : httpRequest;
  while (Date.now() < until) {
    const status = await new Promise<number>((resolve) => {
      const req = request(parsed, { method: "GET", timeout: 15_000, lookup: freshLookup }, (res) => {
        res.resume();
        resolve(res.statusCode ?? 0);
      });
      req.on("timeout", () => req.destroy(new Error("timed out")));
      req.on("error", () => resolve(0));
      req.end();
    });
    if (status > 0 && status < 500) return true;
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
