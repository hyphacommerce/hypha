import type { NextConfig } from "next";

// Standard Next.js configuration only: vinext reads this file, and so would Next.js
// itself. `HYPHA_TARGET=node` (the VPS image): a standalone Node server.
// `globalNotFound`: the root layout is `app/[country]/layout.tsx` (a document's language
// follows the URL), so a path outside any place gets `app/global-not-found.tsx`.
const nextConfig: NextConfig = {
  experimental: { globalNotFound: true },
  ...(process.env.HYPHA_TARGET === "node" ? { output: "standalone" as const } : {}),
};

export default nextConfig;
