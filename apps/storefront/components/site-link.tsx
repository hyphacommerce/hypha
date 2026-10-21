"use client";

// The storefront's link: Next's, starting the navigation bar when followed. Every
// Astryx link, button and card with an `href` renders this (`LinkProvider` in
// app/providers.tsx), and so do the product cards.

import Link from "next/link";
import type { ComponentProps } from "react";

import { useNavigationProgress } from "./navigation-progress";

export function SiteLink({ onNavigate, ...props }: ComponentProps<typeof Link>) {
  const { start } = useNavigationProgress();
  return (
    <Link
      {...props}
      onNavigate={(event) => {
        onNavigate?.(event);
        // Only a navigation this app performs (same origin, not a new tab): Next calls
        // this for those alone.
        start();
      }}
    />
  );
}
