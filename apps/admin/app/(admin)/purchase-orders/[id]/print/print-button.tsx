"use client";

import { Button } from "@astryxdesign/core/Button";

import { useT } from "@/lib/use-t";

/** Opens the browser's print dialog, where the order can also be saved as a PDF. */
export function PrintButton() {
  const t = useT<"purchasing">();
  return (
    <Button label={t("purchasing.print_button")} variant="primary" onClick={() => window.print()} />
  );
}
