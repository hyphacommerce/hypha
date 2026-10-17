import { Skeleton } from "@astryxdesign/core/Skeleton";
import { Suspense } from "react";

import { CurrenciesProvider } from "@/components/currencies";
import { TimeZoneProvider } from "@/components/date-time";
import { DetailPageSkeleton } from "@/components/skeletons";
import { loadCurrencies, loadTimeZone, requireStaff } from "@/lib/hypha";

import { AdminShell, UserMenu } from "./admin-shell";

/**
 * Every page in here needs a signed-in staff member (design note 0011, P4). The frame
 * (top bar and nav) renders at once (design note 0024, section 6); the user's menu and
 * the page stream in, each behind its own skeleton, and every route has a `loading.tsx`
 * of its own for navigations.
 */
export default function AdminLayout({ children }: { children: React.ReactNode }) {
  return (
    <AdminShell
      userMenu={
        <Suspense fallback={<Skeleton width={96} height={28} />}>
          <SignedIn />
        </Suspense>
      }
    >
      <Suspense fallback={<DetailPageSkeleton />}>
        <Providers>{children}</Providers>
      </Suspense>
    </AdminShell>
  );
}

async function SignedIn() {
  const me = await requireStaff();
  return <UserMenu name={me.user.name} email={me.user.email} />;
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
