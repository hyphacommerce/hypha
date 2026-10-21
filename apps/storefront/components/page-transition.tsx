"use client";

// A page change cross-fades (React's `ViewTransition`, over the browser's View
// Transitions API; `.shop-page` in globals.css). The app router navigates inside a
// React transition, which is what activates it; a change the router doesn't animate
// (back and forward, a browser without the API) just swaps. A transition within the
// page (`WITHIN_PAGE`: a listing's results, a product's hero) is its own boundaries'
// (`components/product-grid.tsx`, the product form), not this one's.

import { ViewTransition, WITHIN_PAGE } from "./view-transition";

export function PageTransition({ children }: { children: React.ReactNode }) {
  return (
    <ViewTransition default={{ [WITHIN_PAGE]: "none", default: "shop-page" }}>
      {children}
    </ViewTransition>
  );
}
