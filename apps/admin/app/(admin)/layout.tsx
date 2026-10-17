import { CurrenciesProvider } from "@/components/currencies";
import { TimeZoneProvider } from "@/components/date-time";
import { loadCurrencies, loadTimeZone, requireStaff } from "@/lib/hypha";

import { AdminShell } from "./admin-shell";

/** Every page in here needs a signed-in staff member (design note 0011, P4). */
export default async function AdminLayout({ children }: { children: React.ReactNode }) {
  const [me, currencies, timeZone] = await Promise.all([
    requireStaff(),
    loadCurrencies(),
    loadTimeZone(),
  ]);
  return (
    <CurrenciesProvider currencies={currencies}>
      <TimeZoneProvider timeZone={timeZone}>
        <AdminShell userName={me.user.name} userEmail={me.user.email}>
          {children}
        </AdminShell>
      </TimeZoneProvider>
    </CurrenciesProvider>
  );
}
