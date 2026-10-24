import type { NextConfig } from "next";

// Standard Next.js configuration only: vinext reads this file, and so would Next.js
// itself. `HYPHA_TARGET=node` (the VPS image): a standalone Node server.
// `authInterrupts`: `forbidden()` and `forbidden.tsx`, which a page whose API read
// needs a scope the user's role lacks shows (`loaded` in lib/hypha.ts). vinext supports
// them without the flag; Next.js wants it. Server actions keep the default body limit:
// files go from the browser straight to storage, never through an action.
const nextConfig: NextConfig = {
  ...(process.env.HYPHA_TARGET === "node" ? { output: "standalone" } : {}),
  experimental: { authInterrupts: true },
};

export default nextConfig;
