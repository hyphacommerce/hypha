// Streams a customer's data export to the browser as a JSON download (design note 0032,
// §7.4): the API answers the merchant's data request with one document; this route only
// adds the file name. The staff session stays in its cookie.

import { sessionClient } from "@/lib/hypha";

export async function GET(_request: Request, { params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  const client = await sessionClient();
  const { data, error, response } = await client.GET("/admin/customers/{id}/export", {
    params: { path: { id } },
  });
  if (error) {
    return Response.json(error, { status: response.status });
  }
  return new Response(JSON.stringify(data, null, 2), {
    headers: {
      "content-type": "application/json; charset=utf-8",
      "content-disposition": `attachment; filename="customer-${id}.json"`,
    },
  });
}
