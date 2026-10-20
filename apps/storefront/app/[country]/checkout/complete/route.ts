// Back from a payment provider's page (design note 0031, §5): the cart's payment was
// made there, so the cart is checked out now, and the shopper lands on the order's
// confirmation. Not paid (yet, or at all): back to the checkout, saying why. Only a cart
// paying online is checked out here: a link to this page can't place a cash-on-delivery
// order the shopper never confirmed.

import type { NextRequest } from "next/server";
import { NextResponse } from "next/server";

import { countryOf } from "@/lib/api";
import { checkOutCart, paysOnline } from "@/lib/checkout";
import { link } from "@/lib/site";

/** Per request: it checks out the shopper's cart. */
export const dynamic = "force-dynamic";

/** A redirect with a relative `Location` (see `app/order/[id]/route.ts`). */
function to(path: string): NextResponse {
  return new NextResponse(null, { status: 307, headers: { location: path } });
}

// A provider's return is a GET. It completes only a cart whose money the shopper
// committed at the provider, which is what they asked for; a second visit gets the same
// order.
// react-doctor-disable-next-line react-doctor/nextjs-no-side-effect-in-get-handler
export async function GET(
  _request: NextRequest,
  { params }: { params: Promise<{ country: string }> },
) {
  const country = (await countryOf((await params).country)).country;
  const checkout = link(country, "/checkout");
  if (!(await paysOnline(country))) return to(checkout);
  const result = await checkOutCart({ country, attempt: crypto.randomUUID() });
  if ("order" in result) return to(link(country, `/order/${result.order}/confirmed`));
  return to(`${checkout}?payment=${encodeURIComponent(result.error)}`);
}
