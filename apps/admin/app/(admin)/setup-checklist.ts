// The home page's setup checklist (design note 0021, SU9): worked out by the API on
// every render, never stored. A step the user's scopes can't read counts as not done,
// so the page still renders.

import { sessionClient } from "@/lib/hypha";

/** Which step: the setup wizard (design note 0024, AU11) groups them by these. */
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
  // One read (design note 0025, §6): the API checks each step with the caller's scopes.
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

export async function setupSteps(): Promise<SetupStep[]> {
  const done = await setupDone();
  return [
    {
      key: "store",
      label: "Add your store's details",
      description: "A contact email shoppers can write to, and your legal details.",
      href: "/settings/store",
      isDone: done.store,
    },
    {
      key: "market",
      label: "Create a market",
      description: "The countries you sell to and their currency.",
      href: "/markets",
      isDone: done.market,
    },
    {
      key: "shipping",
      label: "Set up shipping",
      description: "A zone with a rate, so physical products can be delivered.",
      href: "/settings/shipping",
      isDone: done.shipping,
    },
    {
      key: "payment",
      label: "Turn on a payment method",
      description: "Cash on delivery or a bank transfer: checkout is refused without one.",
      href: "/settings/payments",
      isDone: done.payment,
    },
    {
      key: "product",
      label: "Activate a product",
      description: "Drafts can't be sold.",
      href: "/products",
      isDone: done.product,
    },
    {
      key: "stock",
      label: "Stock it",
      description: "Set stock at a location, or leave stock untracked.",
      href: "/products",
      isDone: done.stock,
    },
    {
      key: "publish",
      label: "Publish it to a channel",
      description: "A storefront sells what's published to its channel.",
      href: "/products",
      isDone: done.publish,
    },
    {
      key: "key",
      label: "Create a storefront key",
      description: "The publishable key your storefront calls the API with.",
      href: "/settings/channels",
      isDone: done.key,
    },
  ];
}
