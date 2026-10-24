// Whether the shopper is signed in, for the browser. The session token stays in an
// httpOnly cookie; its lifetime's cookie beside it (`lib/account.ts`) is readable by page
// scripts and set exactly while the token is, so a static page asks the server for the
// shopper's own prices only when there's a session to price for. It holds no secret: a
// forged one costs the shopper one read that finds nothing.

/** The session's lifetime cookie, set with the token and dropped with it. */
export const SESSION_LIFETIME_COOKIE = "customer_session_ttl";

/** Whether the browser's cookies say the shopper is signed in. Client only. */
export function hasSessionFlag(): boolean {
  const name = `${SESSION_LIFETIME_COOKIE}=`;
  return document.cookie.split(";").some((part) => part.trim().startsWith(name));
}
