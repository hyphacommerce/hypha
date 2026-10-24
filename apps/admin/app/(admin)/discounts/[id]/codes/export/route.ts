// A discount's codes as a CSV download (`?batch=` for one batch), passed through from
// the API so the staff session stays in its cookie.

import { sessionClient } from "@/lib/hypha";

export async function GET(request: Request, { params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  const batch = new URL(request.url).searchParams.get("batch")?.trim() || undefined;
  const client = await sessionClient();
  const { data, error, response } = await client.GET("/admin/discounts/{id}/codes/export", {
    params: { path: { id }, query: { batch } },
    parseAs: "text",
  });
  if (data === undefined) return Response.json(error, { status: response.status });
  const name = (batch ?? id).replace(/[^A-Za-z0-9_-]+/g, "-").replace(/^-+|-+$/g, "") || id;
  return new Response(data, {
    headers: {
      "content-type": "text/csv; charset=utf-8",
      "content-disposition": `attachment; filename="discount-codes-${name}.csv"`,
    },
  });
}
