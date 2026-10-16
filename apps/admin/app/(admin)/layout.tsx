import { CurrenciesProvider } from "@/components/currencies";
import { loadCurrencies, requireStaff } from "@/lib/hypha";

import { AdminShell } from "./admin-shell";

/** Every page in here needs a signed-in staff member (design note 0011, P4). */
export default async function AdminLayout({ children }: { children: React.ReactNode }) {
  const [me, currencies] = await Promise.all([requireStaff(), loadCurrencies()]);
  return (
    <CurrenciesProvider currencies={currencies}>
      <AdminShell userName={me.user.name} userEmail={me.user.email}>
        {children}
      </AdminShell>
    </CurrenciesProvider>
  );
}
