"use client";

// The filters in a bottom sheet, for phones: loaded only when a shopper opens it
// (`ListingToolbar`), since wide screens show the facets beside the results instead.

import { BottomSheet } from "@astryxdesign/core/BottomSheet";

import { CloseOnNavigate } from "./close-on-navigate";
import { FacetPanel, type Props } from "./listing-controls";

export default function FilterSheet({
  isOpen,
  onOpenChange,
  ...props
}: Props & { isOpen: boolean; onOpenChange: (open: boolean) => void }) {
  return (
    <>
      {/* Leaving the listing (a product, another category) closes it; filtering here,
        which changes only the query, doesn't. */}
      <CloseOnNavigate pathOnly onNavigate={() => onOpenChange(false)} />
      <BottomSheet label="Filters" isOpen={isOpen} onOpenChange={onOpenChange} height="tall">
        <FacetPanel {...props} onDone={() => onOpenChange(false)} />
      </BottomSheet>
    </>
  );
}
