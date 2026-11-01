// How the admin shows transfers: stock moved between the store's locations.

import type { ISODateString } from "@astryxdesign/core/utils";
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";

export type Transfer = AdminSchemas["TransferResponse"];
export type TransferSummary = AdminSchemas["TransferSummaryResponse"];
export type TransferLine = AdminSchemas["TransferLineResponse"];

/** "TR-1001". */
export function trNumber(number: number): string {
  return `TR-${number}`;
}

/** "Oyster grow kit · Large", or the variant's ID when its names aren't readable. */
export function transferLineLabel(line: TransferLine): string {
  if (!line.product_title) return line.variant;
  return line.variant_title ? `${line.product_title} · ${line.variant_title}` : line.product_title;
}

function isDay(day: string): day is ISODateString {
  return /^\d{4}-\d{2}-\d{2}$/.test(day);
}

/** The calendar day of an RFC 3339 time, as a date input takes it ("2026-03-01"). */
export function dayOf(time: string | null | undefined): ISODateString | undefined {
  const day = time?.slice(0, 10);
  return day && isDay(day) ? day : undefined;
}
