// Back from a payment provider's page (design note 0031, §5): the cart's payment was
// made there, so the cart is checked out now, and the shopper lands on the order's
// confirmation (`placeOrder` redirects there). Not paid (yet, or at all): back to the
// checkout, saying why.

import type { NextRequest } from "next/server";
import { NextResponse } from "next/server";

import { placeOrder } from "@/app/[country]/actions";
import { countryOf } from "@/lib/api";
import { link } from "@/lib/site";

/** Per request: it checks out the shopper's cart. */
export const dynamic = "force-dynamic";

// A provider's return is a GET. Checking out a cart whose money is committed is what the
// shopper asked for, and safe to repeat: a second visit gets the same order.
// react-doctor-disable-next-line react-doctor/nextjs-no-side-effect-in-get-handler
export async function GET(
  _request: NextRequest,
  { params }: { params: Promise<{ country: string }> },
) {
  const country = (await countryOf((await params).country)).country;
  // Success redirects (it throws Next's redirect); an answer is why not.
  const result = await placeOrder({ country, attempt: crypto.randomUUID() });
  const why = result?.error ?? "Your payment didn't go through.";
  const back = `${link(country, "/checkout")}?payment=${encodeURIComponent(why)}`;
  return new NextResponse(null, { status: 307, headers: { location: back } });
}
