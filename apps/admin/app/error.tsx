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
import { useT } from "@/lib/use-t";

export default function PageError({ error, reset }: { error: Error; reset: () => void }) {
  const t = useT();
  const stale = isStaleBuild(error);
  useEffect(() => {
    if (stale) reloadForNewBuild();
    else console.error(error);
  }, [error, stale]);

  return (
    <Center axis="both" padding={6} style={{ minHeight: "100%" }}>
      {stale ? (
        <EmptyState
          title={t("shell.error_stale_title")}
          description={t("shell.error_stale_description")}
          actions={
            <Button
              label={t("common.reload")}
              variant="primary"
              clickAction={() => window.location.reload()}
            />
          }
        />
      ) : (
        <EmptyState
          title={t("common.something_went_wrong")}
          description={t("shell.error_description")}
          actions={<Button label={t("common.retry")} variant="primary" clickAction={reset} />}
        />
      )}
    </Center>
  );
}
