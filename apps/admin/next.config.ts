import type { NextConfig } from "next";

// Standard Next.js configuration only: vinext reads this file, and so would Next.js
// itself. `HYPHA_TARGET=node` (the VPS image): a standalone Node server.
// `authInterrupts`: `forbidden()` and `forbidden.tsx`, which a page whose API read
// needs a scope the user's role lacks shows (`loaded` in lib/hypha.ts). vinext supports
// them without the flag; Next.js wants it. `serverActions.bodySizeLimit`: a price list's
// CSV import goes to its preview action whole, and an import's steps carry up to 1,000
// fixed prices each; 1 MB, the default, is a few thousand rows.
const nextConfig: NextConfig = {
  ...(process.env.HYPHA_TARGET === "node" ? { output: "standalone" } : {}),
  experimental: { authInterrupts: true, serverActions: { bodySizeLimit: "10mb" } },
};

export default nextConfig;
