import { Skeleton } from "@astryxdesign/core/Skeleton";
import { Suspense } from "react";

import { CommandMenu } from "@/components/command-palette";
import { CurrenciesProvider } from "@/components/currencies";
import { TimeZoneProvider } from "@/components/date-time";
import { DetailPageSkeleton } from "@/components/skeletons";
import { loadCurrencies, loadSetup, loadTimeZone, requireStaff } from "@/lib/hypha";

import { AdminShell, SetupNavItem, StoreNav, UserMenu } from "./admin-shell";

/**
 * Every page in here needs a signed-in staff member (design note 0011, P4). The frame
 * (top bar and nav) renders at once (design note 0024, section 6); the user's menu and
 * the page stream in, each behind its own skeleton, and every route has a `loading.tsx`
 * of its own for navigations. The nav and the palette show every page until the user's
 * scopes arrive, then only the pages their role can open.
 */
export default function AdminLayout({ children }: { children: React.ReactNode }) {
  return (
    <AdminShell
      storeNav={
        <Suspense fallback={<StoreNav scopes={null} />}>
          <ScopedStoreNav />
        </Suspense>
      }
      commandMenu={
        <Suspense fallback={<CommandMenu />}>
          <ScopedCommandMenu />
        </Suspense>
      }
      userMenu={
        <Suspense fallback={<Skeleton width={96} height={28} />}>
          <SignedIn />
        </Suspense>
      }
      setupItem={
        <Suspense fallback={null}>
          <SetupItem />
        </Suspense>
      }
    >
      <Suspense fallback={<DetailPageSkeleton />}>
        <Providers>{children}</Providers>
      </Suspense>
    </AdminShell>
  );
}

async function ScopedStoreNav() {
  const me = await requireStaff();
  return <StoreNav scopes={me.scopes} />;
}

async function ScopedCommandMenu() {
  const me = await requireStaff();
  return <CommandMenu scopes={me.scopes} />;
}

async function SignedIn() {
  const me = await requireStaff();
  return <UserMenu name={me.user.name} email={me.user.email} />;
}

/** The setup guide's nav item, while the guide isn't finished (design note 0024, AU11). */
async function SetupItem() {
  const setup = await loadSetup();
  return setup && !setup.isFinished ? <SetupNavItem /> : null;
}

/** What client components read from context: the currency table and the time zone. */
async function Providers({ children }: { children: React.ReactNode }) {
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
