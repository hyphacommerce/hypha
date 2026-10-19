// Pages other sites send the browser to (design note 0029, §4.1: an app sends staff to
// the consent page, `/apps/consent?request=…`). Signed out, the page would lose its
// address on the way through the login, so it goes to `/session-expired` with itself
// as `next`; signed in, the page's address travels with the request
// (`x-hypha-admin-page`), so a session that turns out to be expired comes back here
// too (`sessionExpired` in lib/hypha.ts: such a navigation has no referer of ours).

import { type NextRequest, NextResponse } from "next/server";

/** `SESSION_COOKIE` and `PAGE_HEADER` in lib/hypha.ts (not imported: it's server-only). */
const SESSION_COOKIE = "hypha_admin_session";
const PAGE_HEADER = "x-hypha-admin-page";

export function proxy(request: NextRequest) {
  const { pathname, search } = request.nextUrl;
  const page = `${pathname}${search}`;
  if (!request.cookies.has(SESSION_COOKIE)) {
    // A relative Location: behind a proxy that ends TLS, the request's own URL says http.
    return new NextResponse(null, {
      status: 307,
      headers: { location: `/session-expired?next=${encodeURIComponent(page)}` },
    });
  }
  const headers = new Headers(request.headers);
  headers.set(PAGE_HEADER, page);
  return NextResponse.next({ request: { headers } });
}

export const config = { matcher: ["/apps/consent"] };
