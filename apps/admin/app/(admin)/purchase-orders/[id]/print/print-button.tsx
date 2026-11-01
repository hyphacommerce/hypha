"use client";

import { Button } from "@astryxdesign/core/Button";

/** Opens the browser's print dialog, where the order can also be saved as a PDF. */
export function PrintButton() {
  return <Button label="Print or save as PDF" variant="primary" onClick={() => window.print()} />;
}
