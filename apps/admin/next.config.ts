import type { NextConfig } from "next";

// Standard Next.js configuration only (design note 0011, P2): vinext reads this file,
// and so would Next.js itself.
// `HYPHA_TARGET=node` (the VPS image, design note 0025): a standalone Node server.
// `authInterrupts`: `forbidden()` and `forbidden.tsx`, which a page whose API read
// needs a scope the user's role lacks shows (`loaded` in lib/hypha.ts). vinext supports
// them without the flag; Next.js wants it.
const nextConfig: NextConfig = {
  ...(process.env.HYPHA_TARGET === "node" ? { output: "standalone" } : {}),
  experimental: { authInterrupts: true },
};

export default nextConfig;
