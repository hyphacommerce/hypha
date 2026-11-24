// A discount's phase as a badge, coloured like a price list's: draft, scheduled, active
// or expired. An unknown phase (a newer API) shows as it is.

import { Badge } from "@astryxdesign/core/Badge";

import { type PhaseT, phaseBadge } from "@/lib/price-lists";

export function DiscountStatusBadge({ t, phase }: { t: PhaseT; phase: string }) {
  const { label, variant } = phaseBadge(phase, t);
  return <Badge label={label} variant={variant} />;
}
