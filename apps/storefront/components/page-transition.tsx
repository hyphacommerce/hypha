"use client";

// A page change cross-fades (React's `ViewTransition`, over the browser's View
// Transitions API; `.shop-page` in globals.css). The boundary is keyed by the path, so
// only a navigation to another page exits the old one and enters the new: anything else
// in a transition (a sheet opened by a button, whose clicks Astryx runs in transitions,
// the cart, a listing's filters, a product's options) is an update, which is `none`, or
// those would cross-fade the whole page and flicker. A listing's results and a product's
// hero animate through their own boundaries (`components/product-grid.tsx`, the product
// form). A change the router doesn't animate (back and forward, a browser without the
// API) just swaps.

import { usePathname } from "next/navigation";

import { ViewTransition } from "./view-transition";

export function PageTransition({ children }: { children: React.ReactNode }) {
  const path = usePathname();
  return (
    <ViewTransition key={path} default="none" enter="shop-page" exit="shop-page">
      {children}
    </ViewTransition>
  );
}
