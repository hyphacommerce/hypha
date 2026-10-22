import { CommandMenu } from "@/components/command-palette";
import { CurrenciesProvider } from "@/components/currencies";
import { TimeZoneProvider } from "@/components/date-time";
import { loadCurrencies, loadSetup, loadTimeZone, requireStaff } from "@/lib/hypha";

import { AdminShell, MainNav, SetupNavItem, UserMenu } from "./admin-shell";

/**
 * Every page in here needs a signed-in staff member. The frame reads what it shows in
 * one round trip (the user, for the nav's scopes and the menu; the setup guide's state;
 * the currency table and the store's time zone, which pages' client components read
 * from context) and shares those reads with the page (`perRequest`), so the page's own
 * reads are all a navigation adds. The page then streams in behind its route's
 * `loading.tsx`, the one skeleton a navigation shows: nothing in the frame suspends on
 * its own, since vinext renders the frame again on every navigation and a value that
 * arrived after the page would suspend it a second time.
 */
export default async function AdminLayout({ children }: { children: React.ReactNode }) {
  const [me, setup, currencies, timeZone] = await Promise.all([
    requireStaff(),
    loadSetup(),
    loadCurrencies(),
    loadTimeZone(),
  ]);
  return (
    <AdminShell
      nav={
        <MainNav scopes={me.scopes} setupItem={setup && !setup.isFinished && <SetupNavItem />} />
      }
      commandMenu={<CommandMenu scopes={me.scopes} />}
      userMenu={<UserMenu name={me.user.name} email={me.user.email} />}
    >
      <CurrenciesProvider currencies={currencies}>
        <TimeZoneProvider timeZone={timeZone}>{children}</TimeZoneProvider>
      </CurrenciesProvider>
    </AdminShell>
  );
}
