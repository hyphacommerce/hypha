"use client";

// The shopper's cart in the browser. Pages are static (ISR), so none of them knows the
// cart: this reads it once the page is here (`loadCart`, only when the
// `has_cart_<country>` cookie says there is one, `lib/cart-flag.ts`) and keeps it while
// the shopper adds, changes and removes lines, for the header's count, the drawer and the
// cart page. Every change answers with the cart, so nothing reloads.

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  useTransition,
} from "react";

import {
  type Cart,
  type CartResult,
  addToCart,
  applyDiscountCode,
  loadCart,
  removeDiscountCode,
  setQuantity,
} from "@/app/[country]/actions";
import { CloseOnNavigate } from "@/components/close-on-navigate";
import { WITHIN_PAGE, addTransitionType } from "@/components/view-transition";
import { hasCartFlag } from "@/lib/cart-flag";

type CartContext = {
  country: string;
  /** Minor units of the country's currency. */
  digits: number;
  /** Links to pages in this country (`/hr/p/mug`). */
  base: string;
  cart: Cart | null;
  /** Whether the first read hasn't answered yet. */
  isLoading: boolean;
  isPending: boolean;
  count: number;
  error?: string;
  isOpen: boolean;
  open: () => void;
  close: () => void;
  /** Adds a variant, then opens the drawer; answers with the refusal, if any. */
  add: (variant: string, quantity: number) => Promise<string | undefined>;
  /** Sets a line's quantity; 0 removes it. */
  change: (variant: string, quantity: number) => void;
  /** Adds a discount code; answers with the refusal (an invalid code), if any, for the
   * code field to show by itself rather than as the cart's error. */
  applyCode: (code: string) => Promise<string | undefined>;
  /** Takes a discount code off the cart. */
  removeCode: (code: string) => void;
  /** Reads the cart again from the cookies: after something outside the provider
   * changed it (placing the order forgets the cart, `ReloadCart`). */
  reload: () => Promise<void>;
};

/** Where the change queue starts: nothing to wait for. */
const SETTLED: Promise<unknown> = Promise.resolve();

const Context = createContext<CartContext | null>(null);

export function CartProvider({
  country,
  digits,
  base,
  children,
}: {
  country: string;
  digits: number;
  base: string;
  children: React.ReactNode;
}) {
  const [cart, setCart] = useState<Cart | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [isOpen, setIsOpen] = useState(false);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();

  // Which read is the latest: an older one answering late is dropped.
  const reads = useRef(0);
  const reload = useCallback(async () => {
    const read = ++reads.current;
    let found: Cart | null | undefined = null;
    try {
      // No cart in this country (the flag cookie says so): nothing to read.
      if (hasCartFlag(country)) found = await loadCart(country);
    } catch {
      // The shop's server didn't answer: keep the cart shown (none, on the first read)
      // rather than the cart page's skeleton for good.
      found = undefined;
    }
    if (read !== reads.current) return;
    if (found !== undefined) setCart(found);
    setIsLoading(false);
  }, [country]);

  useEffect(() => {
    // Only the browser knows the cart (the flag cookie, then the server), and a static
    // page can't: with no cart, this sets it at once, one render after hydration.
    // oxlint-disable-next-line react/set-state-in-effect -- reading the browser's cookie
    void reload();
    return () => {
      // Unmounted or another country: whatever is in flight is stale.
      reads.current += 1;
    };
  }, [reload]);

  const add = useCallback(
    async (variant: string, quantity: number) => {
      const result = await addToCart({ country, variant, quantity });
      // Within the page: the caller's transition would otherwise start a view
      // transition, which snapshots the page while the drawer animates open.
      startTransition(() => {
        addTransitionType(WITHIN_PAGE);
        setCart(result.cart);
        setError(result.error);
        if (!result.error) setIsOpen(true);
      });
      return result.error;
    },
    [country],
  );

  // Changes go one at a time: each writes the cart, so two at once would race.
  // Each answers with the cart, shown as it comes; the error is the caller's to place.
  const queue = useRef(SETTLED);
  const write = useCallback(async (send: () => Promise<CartResult>) => {
    const run = queue.current.then(send);
    queue.current = run.catch(() => undefined);
    const result = await run;
    startTransition(() => {
      addTransitionType(WITHIN_PAGE);
      setCart(result.cart);
    });
    return result.error;
  }, []);
  const change = useCallback(
    (variant: string, quantity: number) =>
      startTransition(async () => {
        const failed = await write(() => setQuantity({ country, variant, quantity }));
        startTransition(() => setError(failed));
      }),
    [country, write],
  );
  const applyCode = useCallback(
    (code: string) => write(() => applyDiscountCode({ country, code })),
    [country, write],
  );
  const removeCode = useCallback(
    (code: string) =>
      startTransition(async () => {
        const failed = await write(() => removeDiscountCode({ country, code }));
        startTransition(() => setError(failed));
      }),
    [country, write],
  );

  const value = useMemo<CartContext>(
    () => ({
      country,
      digits,
      base,
      cart,
      isLoading,
      isPending,
      count: cart?.lines.reduce((n, line) => n + line.quantity, 0) ?? 0,
      error,
      isOpen,
      open: () => setIsOpen(true),
      close: () => setIsOpen(false),
      add,
      change,
      applyCode,
      removeCode,
      reload,
    }),
    [
      country,
      digits,
      base,
      cart,
      isLoading,
      isPending,
      error,
      isOpen,
      add,
      change,
      applyCode,
      removeCode,
      reload,
    ],
  );

  return (
    <Context value={value}>
      {/* Following a link in the drawer (a product, the cart, checkout) closes it. */}
      <CloseOnNavigate onNavigate={() => setIsOpen(false)} />
      {children}
    </Context>
  );
}

/**
 * Reads the cart again when it renders: for a page reached after the cart changed on
 * the server, like the order confirmation (placing the order forgot the cart and its
 * flag cookie, but the provider, kept across the navigation, still holds the old cart).
 */
export function ReloadCart() {
  const { reload } = useCart();
  useEffect(() => {
    void reload();
  }, [reload]);
  return null;
}

/** The cart; only inside a country's layout, which provides it. */
export function useCart(): CartContext {
  const value = useContext(Context);
  if (!value) throw new Error("useCart outside the country layout's CartProvider");
  return value;
}
