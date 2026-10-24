// A price list's prices as a CSV download: every product the prices tab's filters
// (`q`, `category`, `fixed=1`) show, on every page, one row per variant. The staff
// session stays in its cookie.

import { loadCurrencies, sessionClient } from "@/lib/hypha";
import { exportPriceCsv } from "@/lib/price-csv";

import { allGridProducts } from "../../grid-data";

export async function GET(request: Request, { params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  const query = new URL(request.url).searchParams;
  const client = await sessionClient();
  const [list, categories, currencies] = await Promise.all([
    client.GET("/admin/price-lists/{id}", { params: { path: { id } } }),
    client.GET("/admin/categories"),
    loadCurrencies(),
  ]);
  if (!list.data) return Response.json(list.error, { status: list.response.status });
  const products = await allGridProducts(client, list.data, categories.data?.items ?? [], {
    q: query.get("q")?.trim() || undefined,
    category: query.get("category") || undefined,
    fixedOnly: query.get("fixed") === "1",
  });
  const name = list.data.name.replace(/[^A-Za-z0-9_-]+/g, "-").replace(/^-+|-+$/g, "") || id;
  return new Response(exportPriceCsv(products, currencies), {
    headers: {
      "content-type": "text/csv; charset=utf-8",
      "content-disposition": `attachment; filename="${name}-prices.csv"`,
    },
  });
}
