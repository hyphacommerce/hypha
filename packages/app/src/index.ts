// @hypha/app: the SDK for Hypha app authors (design note 0029).

export * from "./manifest.ts";
export {
  SIGNATURE_HEADER,
  DEFAULT_TOLERANCE_SECONDS,
  verifySignature,
  signRequest,
  parseSignatureHeader,
  CONTEXT_SIGNATURE_HEADER,
  contextSigningInput,
  verifyContextSignature,
  verifyForwardedRequest,
  type Body,
  type VerifyOptions,
  type SignOptions,
} from "./signature.ts";
export {
  createAuthorizationUrl,
  codeChallenge,
  parseAuthorizationResponse,
  exchangeCode,
  refresh,
  revoke,
  OAuthError,
  type TokenSet,
  type InstallTokens,
  type AuthorizationUrlInput,
  type AuthorizationRequest,
  type AuthorizationResponse,
  type ExchangeCodeInput,
  type RefreshInput,
  type RevokeInput,
} from "./oauth.ts";
export {
  HyphaClient,
  NotInstalledError,
  memoryTokenStore,
  type HyphaClientOptions,
  type TokenStore,
} from "./client.ts";
export {
  defineExtensionHandler,
  extensionHandler,
  ExtensionError,
  EXTENSION_HEADER,
  EXTENSION_VERSION_HEADER,
  IDEMPOTENCY_HEADER,
  STORE_HEADER,
  type PointName,
  type PointRequest,
  type PointResponse,
  type ExtensionContext,
  type ExtensionHandler,
  type ExtensionHandlerFn,
  type ExtensionHandlerOptions,
} from "./extensions.ts";
export type { ExtensionPoints } from "./generated/extensions.ts";
// The hand-written bodies, by name (the generated ones are reached through `PointRequest`).
export type {
  Money,
  PostalAddress,
  ShippingRatesRequest,
  ShippingRatesResponse,
  PaymentsStartRequest,
  PaymentsStartResponse,
  NextAction,
  PaymentActionRequest,
  PaymentActionResponse,
  SearchQueryRequest,
  SearchQueryResponse,
} from "./points.ts";
export {
  verifySessionToken,
  signSessionToken,
  SessionTokenError,
  type SessionClaims,
  type SessionTokenErrorCode,
  type VerifySessionOptions,
} from "./session-token.ts";
