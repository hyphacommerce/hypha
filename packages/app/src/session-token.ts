// Session tokens (design note 0029, §6.3, Q4): an admin asks Hypha for one per embedded
// page or block (`POST /admin/app-extensions/{app}/session`) and hands it to the app
// through the bridge; the app's frontend sends it to the app's backend, which verifies
// it here. A JWT, HS256 with the app's secret, a minute long, its audience the app.
//
// Claims: `iss` (the API's base URL), `aud` (the app's key, `app_…`), `sub` (the staff
// member's user ID), `store`, `scope` (the app's scopes for that staff member,
// space-separated), `iat`, `exp`, `jti`.

import {
  concat,
  fromBase64Url,
  fromUtf8,
  hmacSha256,
  isRecord,
  timingSafeEqual,
  toBase64Url,
  utf8,
} from "./encoding.ts";

export interface SessionClaims {
  /** The Hypha API that issued it. */
  iss: string;
  /** The app's key. */
  aud: string;
  /** The staff member's user ID (`user_…`). */
  sub: string;
  /** The store's ID. */
  store: string;
  /** Space-separated, as in the token. */
  scope: string;
  /** `scope`, split. */
  scopes: string[];
  iat: number;
  exp: number;
  jti: string;
}

export type SessionTokenErrorCode =
  | "malformed"
  | "unsupported_algorithm"
  | "invalid_signature"
  | "expired"
  | "not_yet_valid"
  | "wrong_audience"
  | "wrong_issuer";

export class SessionTokenError extends Error {
  readonly code: SessionTokenErrorCode;
  constructor(code: SessionTokenErrorCode, message: string) {
    super(message);
    this.name = "SessionTokenError";
    this.code = code;
  }
}

export interface VerifySessionOptions {
  /** The app's key: tokens for another app are refused. */
  audience: string;
  /** The API's base URL, when the app knows which deployment to expect. */
  issuer?: string;
  /** Seconds of clock skew allowed. Default 5. */
  clockToleranceSeconds?: number;
  /** Now, in Unix seconds; for tests. */
  now?: number;
}

/** The token's claims, or a `SessionTokenError` saying why it isn't acceptable. */
export async function verifySessionToken(
  token: string,
  secret: string,
  options: VerifySessionOptions,
): Promise<SessionClaims> {
  const parts = token.split(".");
  if (parts.length !== 3) throw new SessionTokenError("malformed", "not a JWT");
  const [headerPart = "", payloadPart = "", signaturePart = ""] = parts;
  const header = decodeJson(headerPart);
  // Only HS256: never `none`, never an algorithm the token picks for itself.
  if (header.alg !== "HS256")
    throw new SessionTokenError("unsupported_algorithm", `alg ${JSON.stringify(header.alg)}`);
  const expected = toBase64Url(await hmacSha256(secret, utf8(`${headerPart}.${payloadPart}`)));
  if (!timingSafeEqual(signaturePart, expected))
    throw new SessionTokenError("invalid_signature", "the signature doesn't match");

  const payload = decodeJson(payloadPart);
  const { iss, aud, sub, store, scope, iat, exp, jti } = payload;
  const nbf = payload.nbf;
  if (
    typeof iss !== "string" ||
    typeof aud !== "string" ||
    typeof sub !== "string" ||
    typeof store !== "string" ||
    typeof iat !== "number" ||
    typeof exp !== "number" ||
    typeof jti !== "string" ||
    (scope !== undefined && typeof scope !== "string") ||
    (nbf !== undefined && typeof nbf !== "number")
  )
    throw new SessionTokenError("malformed", "missing or mistyped claims");

  const now = options.now ?? Math.floor(Date.now() / 1000);
  const skew = options.clockToleranceSeconds ?? 5;
  if (now - skew >= exp) throw new SessionTokenError("expired", "the token has expired");
  if (now + skew < (nbf ?? iat))
    throw new SessionTokenError("not_yet_valid", "issued in the future");
  if (aud !== options.audience)
    throw new SessionTokenError("wrong_audience", `for ${JSON.stringify(aud)}`);
  if (options.issuer !== undefined && iss !== options.issuer)
    throw new SessionTokenError("wrong_issuer", `from ${JSON.stringify(iss)}`);

  const scopeText = scope ?? "";
  return {
    iss,
    aud,
    sub,
    store,
    scope: scopeText,
    scopes: scopeText.split(" ").filter(Boolean),
    iat,
    exp,
    jti,
  };
}

/** Signs a session token as Hypha does. For tests and local tools. */
export async function signSessionToken(
  claims: Omit<SessionClaims, "scopes">,
  secret: string,
): Promise<string> {
  const header = toBase64Url(utf8(JSON.stringify({ alg: "HS256", typ: "JWT" })));
  const payload = toBase64Url(utf8(JSON.stringify(claims)));
  const signature = await hmacSha256(secret, concat(utf8(`${header}.${payload}`)));
  return `${header}.${payload}.${toBase64Url(signature)}`;
}

function decodeJson(part: string): Record<string, unknown> {
  const bytes = fromBase64Url(part);
  if (!bytes) throw new SessionTokenError("malformed", "not base64url");
  let value: unknown;
  try {
    value = JSON.parse(fromUtf8(bytes));
  } catch {
    throw new SessionTokenError("malformed", "not JSON");
  }
  if (!isRecord(value)) throw new SessionTokenError("malformed", "not a JSON object");
  return value;
}
