// An order's link from its emails: the store's `order_url` is
// `https://<storefront>/order/{order}?token={token}` (`hypha deploy` and `pnpm seed`
// set it so). Pages can't set cookies while rendering, so this handler checks the token
// against the order, keeps it in the order's httpOnly cookie (as checkout does), and
// sends the buyer to the confirmation in the order's own country, with no token left in
// the address bar or the history. Country-less, so one pattern serves both routing
// modes: the proxy lets `/order/<id>` through untouched. A wrong or missing token lands
// on the confirmation without one, which shows its "can't show this order" state (or
// the order, if this browser already holds its token).

import { cookies } from "next/headers";
import { type NextRequest, NextResponse } from "next/server";

import { api, loadCountries } from "@/lib/api";
import { COUNTRY_COOKIE } from "@/lib/routing";
import { link, mode, rememberOrder } from "@/lib/site";

/** Per request: it reads the order with the link's token. */
export const dynamic = "force-dynamic";

/**
 * A redirect to a path on this site, with a relative `Location` that the browser
 * resolves against the address it asked for. Not `redirect()`, which makes it absolute
 * from the request this server saw: behind a proxy that ends TLS (a VPS's Caddy), that's
 * `http://`, an extra hop out of HTTPS.
 */
function to(path: string): NextResponse {
  return new NextResponse(null, { status: 307, headers: { location: path } });
}

// An email's link can only be a GET. Its one effect, keeping a token that already reads
// the order, is safe to repeat; a forged visit gives a browser the forger's own order.
// react-doctor-disable-next-line react-doctor/nextjs-no-side-effect-in-get-handler
export async function GET(request: NextRequest, { params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  // The confirmation without a country: the proxy puts it in the shopper's.
  const fallback = `/order/${encodeURIComponent(id)}/confirmed`;
  const token = request.nextUrl.searchParams.get("token");
  if (!token) return to(fallback);
  const { data: order } = await api().GET("/storefront/orders/{id}", {
    params: { path: { id }, query: { token } },
  });
  if (!order) return to(fallback);
  await rememberOrder(order.id, token);
  const sold = (await loadCountries()).some((c) => c.country === order.country);
  // A country no market sells to any more has no folder: the shopper's one, then.
  if (!sold) return to(fallback);
  // A single tree takes the country from its cookie: the order's, so its page shows
  // the order's country and money.
  if (mode() === "single") {
    (await cookies()).set(COUNTRY_COOKIE, order.country, {
      sameSite: "lax",
      path: "/",
      maxAge: 60 * 60 * 24 * 365,
    });
  }
  return to(link(order.country, `/order/${order.id}/confirmed`));
}
