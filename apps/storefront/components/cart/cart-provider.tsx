"use client";

// The shopper's cart in the browser. Pages are static (ISR), so none of them knows the
// cart: this reads it once the page is here (`loadCart`, only when the
// `has_cart_<country>` cookie says there is one, `lib/cart-flag.ts`) and keeps it while
// the shopper adds, changes and removes lines, for the header's count, the drawer and the
// cart page. Every change answers with the cart, so nothing reloads, except a page that
// shows the cart from the server (checkout, `FollowCart`): it re-renders in the change's
// answer.

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
import { useLocale } from "@/components/locale";
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
  /** Makes every change re-render the page too, until the returned function is called
   * (`FollowCart`). */
  follow: () => () => void;
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
  // The page's place: actions answer in its language.
  const locale = useLocale();

  // Changes go one at a time: each writes the cart, so two at once would race for its
  // version. A read waits its turn too: it may write (`loadCart` moves a cart in
  // another language to the page's).
  const queue = useRef(SETTLED);

  // How many parts of the page show the cart as the server rendered it (`FollowCart`):
  // while any does, each change asks its action to re-render the page in its answer. A
  // ref, read when a change is sent: following changes nothing shown.
  const followers = useRef(0);
  const follow = useCallback(() => {
    followers.current += 1;
    return () => {
      followers.current -= 1;
    };
  }, []);

  // Which read is the latest: an older one answering late is dropped.
  const reads = useRef(0);
  const reload = useCallback(async () => {
    const read = ++reads.current;
    let found: Cart | null | undefined = null;
    try {
      // No cart in this country (the flag cookie says so): nothing to read.
      if (hasCartFlag(country)) {
        const run = queue.current.then(() => loadCart(locale));
        queue.current = run.catch(() => undefined);
        found = await run;
      }
    } catch {
      // The shop's server didn't answer: keep the cart shown (none, on the first read)
      // rather than the cart page's skeleton for good.
      found = undefined;
    }
    if (read !== reads.current) return;
    if (found !== undefined) setCart(found);
    setIsLoading(false);
  }, [country, locale]);

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
      const run = queue.current.then(() =>
        addToCart({ locale, variant, quantity, rerender: followers.current > 0 }),
      );
      queue.current = run.catch(() => undefined);
      const result = await run;
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
    [locale],
  );

  // Each change answers with the cart, shown as it comes; the error is the caller's to
  // place.
  const write = useCallback(async (send: (rerender: boolean) => Promise<CartResult>) => {
    const run = queue.current.then(() => send(followers.current > 0));
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
        const failed = await write((rerender) =>
          setQuantity({ locale, variant, quantity, rerender }),
        );
        startTransition(() => setError(failed));
      }),
    [locale, write],
  );
  const applyCode = useCallback(
    (code: string) => write((rerender) => applyDiscountCode({ locale, code, rerender })),
    [locale, write],
  );
  const removeCode = useCallback(
    (code: string) =>
      startTransition(async () => {
        const failed = await write((rerender) => removeDiscountCode({ locale, code, rerender }));
        startTransition(() => setError(failed));
      }),
    [locale, write],
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
      follow,
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
      follow,
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

/**
 * Rendered by a page that shows the cart as the server rendered it (checkout's summary
 * and steps): a change made through the provider (the drawer's lines and codes) then
 * re-renders the page in the change's answer, so it never shows an old total.
 */
export function FollowCart() {
  const { follow } = useCart();
  useEffect(() => follow(), [follow]);
  return null;
}

/** The cart; only inside a country's layout, which provides it. */
export function useCart(): CartContext {
  const value = useContext(Context);
  if (!value) throw new Error("useCart outside the country layout's CartProvider");
  return value;
}
