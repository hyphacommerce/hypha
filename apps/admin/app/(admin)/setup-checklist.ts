// The home page's setup checklist: worked out by the API on every render, never
// stored. A step the user's scopes can't read counts as not done, so the page still
// renders.

import { sessionClient } from "@/lib/hypha";
import type { TFor } from "@/lib/messages";

/** Which step: the setup wizard groups them by these. */
export type SetupKey =
  | "store"
  | "market"
  | "shipping"
  | "payment"
  | "product"
  | "stock"
  | "publish"
  | "key";

export type SetupStep = {
  key: SetupKey;
  label: string;
  description: string;
  href: string;
  isDone: boolean;
};

/** Which steps are done. */
export async function setupDone(): Promise<Record<SetupKey, boolean>> {
  const client = await sessionClient();
  // One read: the API checks each step with the caller's scopes.
  const { data } = await client.GET("/admin/setup/checklist");
  return (
    data ?? {
      store: false,
      market: false,
      shipping: false,
      payment: false,
      product: false,
      stock: false,
      publish: false,
      key: false,
    }
  );
}

/** The steps in order, each with the screen it's done on. */
const STEPS: { key: SetupKey; href: string }[] = [
  { key: "store", href: "/settings/store" },
  { key: "market", href: "/markets" },
  { key: "shipping", href: "/settings/shipping" },
  { key: "payment", href: "/settings/payments" },
  { key: "product", href: "/products" },
  { key: "stock", href: "/products" },
  { key: "publish", href: "/products" },
  { key: "key", href: "/settings/channels" },
];

/** The checklist as `t`'s language words it, each step done or not. */
export async function setupSteps(t: TFor<"setup">): Promise<SetupStep[]> {
  const done = await setupDone();
  return STEPS.map(({ key, href }) => ({
    key,
    label: t(`setup.checklist_${key}_label`),
    description: t(`setup.checklist_${key}_description`),
    href,
    isDone: done[key],
  }));
}
