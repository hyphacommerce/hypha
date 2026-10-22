// Tracking numbers as the order page edits them: shared by the dialogs, which
// check them, and the server actions, which send them.

/** One row of the form. Every field is text; blanks are left out when sent. */
export type TrackingInput = { carrier: string; number: string; url: string };

export const NO_TRACKING: TrackingInput = { carrier: "", number: "", url: "" };

/** The API takes at most 10 per shipment. */
export const MAX_TRACKING = 10;

function isBlank(row: TrackingInput): boolean {
  return !row.carrier.trim() && !row.number.trim() && !row.url.trim();
}

/**
 * What's wrong with the rows, if anything: a carrier or link needs its number, so none
 * is dropped without a word. Fully blank rows are fine (they're left out).
 */
export function trackingProblem(rows: TrackingInput[]): string | undefined {
  if (rows.some((row) => !isBlank(row) && !row.number.trim())) {
    return "Enter the tracking number for each carrier or link, or clear them.";
  }
  return undefined;
}

/** The rows as the API takes them, blank ones left out. */
export function trackingBody(rows: TrackingInput[]) {
  return rows
    .filter((row) => !isBlank(row))
    .map((row) => ({
      number: row.number.trim(),
      carrier: row.carrier.trim() || null,
      url: row.url.trim() || null,
    }));
}
