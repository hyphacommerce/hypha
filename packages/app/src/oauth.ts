// The app's side of installing (design note 0029, §4.1, AP5): OAuth 2.1, authorization
// code with PKCE (S256). The app's client ID is its manifest URL, so there's nothing to
// register: send the browser to `createAuthorizationUrl`, keep the verifier and state
// (a cookie, the app's session), and exchange the code on the redirect.

import { isRecord, randomBytes, sha256, toBase64Url, utf8 } from "./encoding.ts";

/** Tokens for one installation (one Hypha deployment). */
export interface TokenSet {
  /** `hypha_at_…`, an hour. */
  accessToken: string;
  /** `hypha_rt_…`: rotates on every use; the old one is dead once used. */
  refreshToken: string;
  /** When the access token expires, in milliseconds since the epoch. */
  expiresAt: number;
  /** The scopes granted, which may be fewer than asked for. */
  scopes: string[];
  /**
   * The store the tokens are for (`store_…`), as webhooks and extension calls name it in
   * `Hypha-Store`: key the installation by it.
   */
  store: string;
  /** The API's base URL (the authorization server's `issuer`): where to call and refresh. */
  issuer: string;
}

/** What the first code exchange returns: the tokens and, on first install, the app secret. */
export interface InstallTokens extends TokenSet {
  /**
   * The app's signing secret, for verifying webhooks, extension calls and session
   * tokens. Sent once, on first install: store it with the installation.
   */
  appSecret?: string;
}

/** An RFC 6749 error from the token endpoint (`invalid_grant`, …), or a transport failure. */
export class OAuthError extends Error {
  readonly code: string;
  readonly description: string | undefined;
  readonly status: number;
  constructor(code: string, description: string | undefined, status: number) {
    super(description ? `${code}: ${description}` : code);
    this.name = "OAuthError";
    this.code = code;
    this.description = description;
    this.status = status;
  }
}

export interface AuthorizationUrlInput {
  /** The Hypha API's base URL, e.g. `https://api.shop.example`. */
  apiUrl: string;
  /** The app's manifest URL: its OAuth client ID. */
  manifestUrl: string;
  /** One of the manifest's `redirect_uris`, exactly. */
  redirectUri: string;
  scopes: readonly string[];
  /** Ties the redirect to this browser; a random one is made when left out. */
  state?: string;
}

export interface AuthorizationRequest {
  url: string;
  /** Keep until the redirect: `exchangeCode` needs it. */
  codeVerifier: string;
  /** Compare with the redirect's `state` before exchanging. */
  state: string;
}

/** The URL to send a staff member's browser to, with a fresh PKCE verifier. */
export async function createAuthorizationUrl(
  input: AuthorizationUrlInput,
): Promise<AuthorizationRequest> {
  const codeVerifier = toBase64Url(randomBytes(32));
  const state = input.state ?? toBase64Url(randomBytes(16));
  const url = new URL("/oauth/authorize", input.apiUrl);
  url.search = new URLSearchParams({
    response_type: "code",
    client_id: input.manifestUrl,
    redirect_uri: input.redirectUri,
    scope: input.scopes.join(" "),
    state,
    code_challenge: await codeChallenge(codeVerifier),
    code_challenge_method: "S256",
  }).toString();
  return { url: url.toString(), codeVerifier, state };
}

/** PKCE's S256 challenge: base64url of the verifier's SHA-256. */
export async function codeChallenge(verifier: string): Promise<string> {
  return toBase64Url(await sha256(utf8(verifier)));
}

/** What the redirect brought back: the code, or the staff member's refusal. */
export type AuthorizationResponse =
  | { ok: true; code: string; state: string }
  | { ok: false; error: string; description?: string; state?: string };

/** Reads the redirect URI's query; check `state` against the one you kept. */
export function parseAuthorizationResponse(url: string | URL): AuthorizationResponse {
  const query = new URL(url).searchParams;
  const error = query.get("error");
  const state = query.get("state") ?? undefined;
  if (error) {
    const description = query.get("error_description") ?? undefined;
    return {
      ok: false,
      error,
      ...(description ? { description } : {}),
      ...(state ? { state } : {}),
    };
  }
  const code = query.get("code");
  if (!code || !state) return { ok: false, error: "invalid_request", ...(state ? { state } : {}) };
  return { ok: true, code, state };
}

interface Transport {
  apiUrl: string;
  manifestUrl: string;
  fetch?: typeof globalThis.fetch;
  /** Now in milliseconds; for tests. */
  now?: () => number;
}

export interface ExchangeCodeInput extends Transport {
  code: string;
  codeVerifier: string;
  redirectUri: string;
}

/** Trades the redirect's code (60 seconds, single use) for tokens. */
export async function exchangeCode(input: ExchangeCodeInput): Promise<InstallTokens> {
  const body = await tokenRequest(input, {
    grant_type: "authorization_code",
    code: input.code,
    redirect_uri: input.redirectUri,
    code_verifier: input.codeVerifier,
  });
  const tokens = toTokenSet(body, input.now);
  const secret = body.app_secret;
  return typeof secret === "string" ? { ...tokens, appSecret: secret } : tokens;
}

export interface RefreshInput extends Transport {
  refreshToken: string;
}

/**
 * Trades a refresh token for new tokens. The refresh token rotates: store the returned
 * one and drop the old, which Hypha now treats as stolen if it's used again (that
 * revokes the whole grant). Never refresh the same token twice concurrently.
 */
export async function refresh(input: RefreshInput): Promise<TokenSet> {
  const body = await tokenRequest(input, {
    grant_type: "refresh_token",
    refresh_token: input.refreshToken,
  });
  return toTokenSet(body, input.now);
}

export interface RevokeInput extends Transport {
  token: string;
}

/** Revokes a token (RFC 7009); revoking a refresh token ends the grant's tokens. */
export async function revoke(input: RevokeInput): Promise<void> {
  const fetch = input.fetch ?? globalThis.fetch;
  const response = await fetch(new URL("/oauth/revoke", input.apiUrl), {
    method: "POST",
    headers: { "content-type": "application/x-www-form-urlencoded" },
    body: new URLSearchParams({ token: input.token, client_id: input.manifestUrl }),
  });
  if (!response.ok) throw await oauthError(response);
}

async function tokenRequest(
  input: Transport,
  params: Record<string, string>,
): Promise<Record<string, unknown>> {
  const fetch = input.fetch ?? globalThis.fetch;
  const response = await fetch(new URL("/oauth/token", input.apiUrl), {
    method: "POST",
    headers: {
      "content-type": "application/x-www-form-urlencoded",
      accept: "application/json",
    },
    body: new URLSearchParams({ ...params, client_id: input.manifestUrl }),
  });
  if (!response.ok) throw await oauthError(response);
  const body: unknown = await response.json().catch(() => undefined);
  if (!isRecord(body))
    throw new OAuthError("invalid_response", "not a JSON object", response.status);
  return body;
}

function toTokenSet(body: Record<string, unknown>, now: (() => number) | undefined): TokenSet {
  const { access_token, refresh_token, expires_in, scope, store, issuer } = body;
  if (
    typeof access_token !== "string" ||
    typeof refresh_token !== "string" ||
    typeof expires_in !== "number" ||
    typeof store !== "string" ||
    typeof issuer !== "string"
  ) {
    throw new OAuthError(
      "invalid_response",
      "missing access_token, refresh_token, expires_in, store or issuer",
      200,
    );
  }
  return {
    accessToken: access_token,
    refreshToken: refresh_token,
    expiresAt: (now ?? Date.now)() + expires_in * 1000,
    scopes: typeof scope === "string" ? scope.split(" ").filter(Boolean) : [],
    store,
    issuer,
  };
}

async function oauthError(response: Response): Promise<OAuthError> {
  const body: unknown = await response.json().catch(() => undefined);
  if (isRecord(body) && typeof body.error === "string") {
    const description =
      typeof body.error_description === "string" ? body.error_description : undefined;
    return new OAuthError(body.error, description, response.status);
  }
  return new OAuthError("server_error", `HTTP ${response.status}`, response.status);
}
