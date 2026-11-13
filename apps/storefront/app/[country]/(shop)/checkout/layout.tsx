import { ClientMessages } from "@/components/messages";
import { pageT } from "@/lib/i18n";

/** The checkout's forms read its messages and the address form's: sent to the browser
 * here, on top of what every page gets. */
export default async function Layout({
  children,
  params,
}: {
  children: React.ReactNode;
  params: Promise<{ country: string }>;
}) {
  const t = await pageT((await params).country);
  return (
    <ClientMessages t={t} namespaces={["checkout", "address"]}>
      {children}
    </ClientMessages>
  );
}
