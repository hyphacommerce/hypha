"use client";

// Times in the store's time zone and the staff member's language: the admin layout loads
// the zone once (`loadTimeZone`) and provides it here, so every page and table shows
// times the same way without fetching anything itself.

import { createContext, useContext } from "react";

import { useFormat } from "@/lib/use-format";

const TimeZoneContext = createContext("UTC");

export function TimeZoneProvider({
  timeZone,
  children,
}: {
  timeZone: string;
  children: React.ReactNode;
}) {
  return <TimeZoneContext value={timeZone}>{children}</TimeZoneContext>;
}

/** The store's time zone; UTC outside the admin layout. */
export function useTimeZone(): string {
  return useContext(TimeZoneContext);
}

/**
 * A time from the API (RFC 3339), in the store's time zone: `short` in tables, `long`
 * (with the zone) on detail pages. Hovering shows the long form.
 */
export function DateTime({ value, short = false }: { value: string; short?: boolean }) {
  const format = useFormat();
  const long = format.dateTime(value, "long");
  return (
    <time dateTime={value} title={long}>
      {short ? format.dateTime(value, "short") : long}
    </time>
  );
}

/**
 * A calendar date the API keeps as noon UTC (an expected date): the day in UTC, never
 * the store's zone, which would move it to the next day east of UTC+12. No time of day:
 * `short` "11 Nov" in tables, else "11 Nov 2026".
 */
export function CalendarDate({ value, short = false }: { value: string; short?: boolean }) {
  const format = useFormat();
  return (
    <time dateTime={value.slice(0, 10)} title={format.date(value, "long")}>
      {format.date(value, short ? "short" : "long")}
    </time>
  );
}
