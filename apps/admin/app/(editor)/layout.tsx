import { CurrenciesProvider } from "@/components/currencies";
import { TimeZoneProvider } from "@/components/date-time";
import { loadCurrencies, loadTimeZone, requireStaff } from "@/lib/hypha";

/**
 * Full-screen tools, outside the admin's frame: the whole window is the work (the price
 * editor). Signed-in staff only, as in the frame, and the same context its client
 * components read (the currency table, the store's time zone), shared with the page
 * (`perRequest`). Each page brings its own way back.
 */
export default async function EditorLayout({ children }: { children: React.ReactNode }) {
  const [, currencies, timeZone] = await Promise.all([
    requireStaff(),
    loadCurrencies(),
    loadTimeZone(),
  ]);
  return (
    <CurrenciesProvider currencies={currencies}>
      <TimeZoneProvider timeZone={timeZone}>{children}</TimeZoneProvider>
    </CurrenciesProvider>
  );
}
