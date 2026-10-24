// A discount's phase as a badge, coloured like a price list's: draft, scheduled, active
// or expired. An unknown phase (a newer API) shows as it is.

import { Badge } from "@astryxdesign/core/Badge";

import { phaseBadge } from "@/lib/price-lists";

export function DiscountStatusBadge({ phase }: { phase: string }) {
  const { label, variant } = phaseBadge(phase);
  return <Badge label={label} variant={variant} />;
}
