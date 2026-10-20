"use client";

// The last error boundary, for a failure in the root layout itself: it renders its own
// document. Code from an earlier deploy reloads the page once (`lib/stale-build.ts`).

import { useEffect } from "react";

import { isStaleBuild, reloadForNewBuild } from "@/lib/stale-build";

export default function GlobalError({ error, reset }: { error: Error; reset: () => void }) {
  useEffect(() => {
    if (!(isStaleBuild(error) && reloadForNewBuild())) console.error(error);
  }, [error]);

  return (
    <html lang="en">
      <body style={{ fontFamily: "system-ui, sans-serif", padding: 32 }}>
        <h1 style={{ fontSize: 20 }}>Something went wrong</h1>
        <p>The admin ran into a problem. Reload the page to try again.</p>
        <button type="button" onClick={reset}>
          Try again
        </button>
      </body>
    </html>
  );
}
