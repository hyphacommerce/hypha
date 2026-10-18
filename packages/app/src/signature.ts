// Hypha signs every call it makes to an app (webhooks, extension points, the proxy) like
// Stripe does (design notes 0015 §5, 0016 §8, 0029 §15): `Hypha-Signature:
// t=<unix seconds>,v1=<hex HMAC-SHA256 of "<t>.<body>">`, keyed with the app's secret.

import { bodyBytes, concat, hmacSha256, timingSafeEqual, toHex, utf8 } from "./encoding.ts";

/** The header Hypha's signature travels in. */
export const SIGNATURE_HEADER = "Hypha-Signature";

/** How far a signature's timestamp may be from now by default: older ones are replays. */
export const DEFAULT_TOLERANCE_SECONDS = 5 * 60;

export type Body = string | Uint8Array | ArrayBuffer;

export interface VerifyOptions {
  /** Seconds a timestamp may differ from now (either way). Default 300. */
  toleranceSeconds?: number;
  /** Now, in Unix seconds; for tests. */
  now?: number;
}

/**
 * Whether `header` (the `Hypha-Signature` value) signs `rawBody` with `secret`, within
 * the tolerance. Pass the body exactly as received: re-serialized JSON won't match.
 * Several `v1` entries are accepted (any one matching), so a secret can be rotated.
 */
export async function verifySignature(
  secret: string,
  header: string | null | undefined,
  rawBody: Body,
  options: VerifyOptions = {},
): Promise<boolean> {
  if (!header || !secret) return false;
  const parsed = parseSignatureHeader(header);
  if (!parsed) return false;
  const tolerance = options.toleranceSeconds ?? DEFAULT_TOLERANCE_SECONDS;
  const now = options.now ?? Math.floor(Date.now() / 1000);
  if (Math.abs(now - parsed.timestamp) > tolerance) return false;
  const expected = await digest(secret, parsed.timestamp, rawBody);
  let matched = false;
  // Every candidate is compared, so the time taken doesn't tell which one matched.
  for (const candidate of parsed.signatures) {
    if (timingSafeEqual(candidate, expected)) matched = true;
  }
  return matched;
}

export interface SignOptions {
  /** Unix seconds; now by default. */
  timestamp?: number;
}

/** The `Hypha-Signature` value for `body`, as Hypha computes it. For tests and tools. */
export async function signRequest(
  secret: string,
  body: Body,
  options: SignOptions = {},
): Promise<string> {
  const timestamp = options.timestamp ?? Math.floor(Date.now() / 1000);
  return `t=${timestamp},v1=${await digest(secret, timestamp, body)}`;
}

/** `t` and every `v1` of a signature header; `undefined` when it has no valid `t` or `v1`. */
export function parseSignatureHeader(
  header: string,
): { timestamp: number; signatures: string[] } | undefined {
  let timestamp: number | undefined;
  const signatures: string[] = [];
  for (const part of header.split(",")) {
    const at = part.indexOf("=");
    if (at < 0) continue;
    const key = part.slice(0, at).trim();
    const value = part.slice(at + 1).trim();
    if (key === "t" && /^\d+$/.test(value)) timestamp = Number(value);
    else if (key === "v1" && /^[0-9a-f]{64}$/.test(value)) signatures.push(value);
  }
  if (timestamp === undefined || !Number.isSafeInteger(timestamp) || signatures.length === 0)
    return undefined;
  return { timestamp, signatures };
}

async function digest(secret: string, timestamp: number, body: Body): Promise<string> {
  return toHex(await hmacSha256(secret, concat(utf8(`${timestamp}.`), bodyBytes(body))));
}
