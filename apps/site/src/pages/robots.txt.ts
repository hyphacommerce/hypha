import type { APIRoute } from "astro";

// Everything is public, AI crawlers included: being found and read by agents is the point.
export const GET: APIRoute = ({ site }) =>
  new Response(
    ["User-agent: *", "Allow: /", "", `Sitemap: ${new URL("sitemap-index.xml", site)}`, ""].join(
      "\n",
    ),
    { headers: { "Content-Type": "text/plain; charset=utf-8" } },
  );
