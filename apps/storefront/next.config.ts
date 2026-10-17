import type { NextConfig } from "next";

// Standard Next.js configuration only (design notes 0011, P2; 0023, SF1): vinext reads
// this file, and so would Next.js itself.
// `HYPHA_TARGET=node` (the VPS image, design note 0025): a standalone Node server.
const nextConfig: NextConfig = process.env.HYPHA_TARGET === "node" ? { output: "standalone" } : {};

export default nextConfig;
