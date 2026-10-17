"use client";

// The shopper's cart in the browser (design note 0024, §7). Pages are static (ISR), so
// none of them knows the cart: this reads it once the page is here (`loadCart`) and
// keeps it while the shopper adds, changes and removes lines, for the header's count,
// the drawer and the cart page. Every change answers with the cart, so nothing reloads.

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

import { type Cart, addToCart, loadCart, setQuantity } from "@/app/[country]/actions";

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
};

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

  useEffect(() => {
    let current = true;
    loadCart(country).then((found) => {
      if (!current) return;
      setCart(found);
      setIsLoading(false);
    });
    return () => {
      current = false;
    };
  }, [country]);

  const add = useCallback(
    async (variant: string, quantity: number) => {
      const result = await addToCart({ country, variant, quantity });
      setCart(result.cart);
      setError(result.error);
      if (!result.error) setIsOpen(true);
      return result.error;
    },
    [country],
  );

  // Changes go one at a time: each writes the cart, so two at once would race.
  const queue = useRef<Promise<unknown>>(Promise.resolve());
  const change = useCallback(
    (variant: string, quantity: number) =>
      startTransition(async () => {
        const run = queue.current.then(() => setQuantity({ country, variant, quantity }));
        queue.current = run.catch(() => undefined);
        const result = await run;
        setCart(result.cart);
        setError(result.error);
      }),
    [country],
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
    }),
    [country, digits, base, cart, isLoading, isPending, error, isOpen, add, change],
  );

  return <Context value={value}>{children}</Context>;
}

/** The cart; only inside a country's layout, which provides it. */
export function useCart(): CartContext {
  const value = useContext(Context);
  if (!value) throw new Error("useCart outside the country layout's CartProvider");
  return value;
}
