"use client";

// The admin's error boundary: a page that failed while rendering in the browser. Code
// from an earlier deploy that can't be loaded any more (`lib/stale-build.ts`) reloads the
// page once, onto the new build (with a button, should that reload be skipped); anything
// else is shown with a way to try again.

import { Button } from "@astryxdesign/core/Button";
import { Center } from "@astryxdesign/core/Center";
import { EmptyState } from "@astryxdesign/core/EmptyState";
import { useEffect } from "react";

import { isStaleBuild, reloadForNewBuild } from "@/lib/stale-build";

export default function PageError({ error, reset }: { error: Error; reset: () => void }) {
  const stale = isStaleBuild(error);
  useEffect(() => {
    if (stale) reloadForNewBuild();
    else console.error(error);
  }, [error, stale]);

  return (
    <Center axis="both" padding={6} style={{ minHeight: "100%" }}>
      {stale ? (
        <EmptyState
          title="A new version of the admin is out"
          description="Loading it… If this page stays, reload it."
          actions={
            <Button label="Reload" variant="primary" clickAction={() => window.location.reload()} />
          }
        />
      ) : (
        <EmptyState
          title="Something went wrong"
          description="This page ran into a problem."
          actions={<Button label="Try again" variant="primary" clickAction={reset} />}
        />
      )}
    </Center>
  );
}
