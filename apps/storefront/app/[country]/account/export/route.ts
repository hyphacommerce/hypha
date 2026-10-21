// The account's data as a download (design note 0034, §3): the API's export, as the
// signed-in shopper, handed over as a JSON file.

import { type NextRequest, NextResponse } from "next/server";

import { accountApi } from "@/lib/account";

export const dynamic = "force-dynamic";

// A download is a GET; it changes nothing but records that the person asked.
// react-doctor-disable-next-line react-doctor/nextjs-no-side-effect-in-get-handler
export async function GET(_request: NextRequest) {
  const client = await accountApi();
  if (!client) return new NextResponse(null, { status: 401 });
  const { data, response } = await client.GET("/storefront/account/export");
  if (!data) return new NextResponse(null, { status: response.status });
  return new NextResponse(JSON.stringify(data, null, 2), {
    headers: {
      "content-type": "application/json",
      "content-disposition": `attachment; filename="my-data-${data.exported_at.slice(0, 10)}.json"`,
      "cache-control": "no-store",
    },
  });
}
