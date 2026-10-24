"use client";

// What the product costs where it's sold: per market, each variant's price, the price
// struck through beside it, the list that set it, its quantity breaks, and (on demand)
// every list that competed and why it lost. A quantity previews the breaks, and a
// customer group what its signed-in customers pay.

import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Card } from "@astryxdesign/core/Card";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { Selector } from "@astryxdesign/core/Selector";
import { Switch } from "@astryxdesign/core/Switch";
import {
  Table,
  TableBody,
  TableCell,
  TableHeader,
  TableHeaderCell,
  TableRow,
} from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { useEffect, useEffectEvent, useState, useTransition } from "react";

import { useCurrencies } from "@/components/currencies";
import { formatMoney } from "@/lib/format";
import { candidateReason } from "@/lib/price-lists";

import { type ProductPrices, productPrices } from "../actions";

type MarketPrices = ProductPrices["markets"][number];

/** The group selector's value for a shopper who isn't signed in. */
const GUEST = "guest";

/** A whole number from 1 up, or null. */
function parseQuantity(text: string): number | null {
  const n = Number(text.trim());
  return Number.isSafeInteger(n) && n >= 1 && n <= 1_000_000 ? n : null;
}

export function PricesCard({
  productId,
  initial,
  markets,
  groups,
  variantTitles,
}: {
  productId: string;
  /** The prices at quantity 1, read with the page. */
  initial: ProductPrices;
  /** Market names by ID. */
  markets: Record<string, string>;
  /** The customer groups to price for; none without `customers:read` (no selector). */
  groups: { value: string; label: string }[];
  /** "M / Blue" by variant ID; null for a product without options. */
  variantTitles: Record<string, string | null>;
}) {
  const [prices, setPrices] = useState(initial);
  const [quantity, setQuantity] = useState("1");
  const [group, setGroup] = useState(GUEST);
  // What the prices shown were last asked for (an answer that failed included, so it
  // isn't asked again until something changes).
  const [asked, setAsked] = useState({ quantity: initial.quantity, group: GUEST });
  const [isWhyShown, setIsWhyShown] = useState(false);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const wanted = parseQuantity(quantity);

  const read = useEffectEvent((n: number, g: string) =>
    startTransition(async () => {
      const result = await productPrices(productId, n, g === GUEST ? undefined : g);
      if (result.prices) setPrices(result.prices);
      setAsked({ quantity: n, group: g });
      setError(result.error);
    }),
  );
  useEffect(() => {
    if (wanted === null || (wanted === asked.quantity && group === asked.group)) return;
    // A typed quantity waits for the typing to stop; a group is read at once.
    const timer = setTimeout(() => read(wanted, group), group === asked.group ? 350 : 0);
    return () => clearTimeout(timer);
  }, [wanted, group, asked]);

  return (
    <Card>
      <VStack gap={4}>
        <HStack gap={3} hAlign="between" vAlign="end" wrap="wrap">
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              Prices
            </Text>
            <Text type="supporting" color="secondary">
              What shoppers in each market pay now: the lowest live price among the market&apos;s
              price lists, or the base price where none prices a variant.
            </Text>
          </VStack>
          <HStack gap={3} vAlign="end" wrap="wrap">
            {groups.length > 0 && (
              <Selector
                label="Customer"
                description="Price lists for a group count once its customers sign in."
                options={[{ value: GUEST, label: "Not signed in" }, ...groups]}
                value={group}
                onChange={setGroup}
                width={220}
              />
            )}
            <TextInput
              label="Quantity"
              description="Of each variant, for quantity breaks."
              value={quantity}
              onChange={setQuantity}
              status={
                wanted === null ? { type: "error", message: "A whole number from 1." } : undefined
              }
              isLoading={isPending}
              width={160}
            />
            <Switch label="Show every list" value={isWhyShown} onChange={setIsWhyShown} />
          </HStack>
        </HStack>
        {error && <Banner status="error" title={error} />}
        {prices.markets.length === 0 ? (
          <Text color="secondary">No market sells it yet: add it to a market&apos;s catalog.</Text>
        ) : (
          <div
            className="pending-content"
            aria-busy={isPending}
            data-pending={isPending || undefined}
          >
            <VStack gap={5}>
              {prices.markets.map((m) => (
                <MarketTable
                  key={m.market}
                  market={m}
                  name={markets[m.market] ?? m.market}
                  variantTitles={variantTitles}
                  isWhyShown={isWhyShown}
                />
              ))}
            </VStack>
          </div>
        )}
      </VStack>
    </Card>
  );
}

function MarketTable({
  market,
  name,
  variantTitles,
  isWhyShown,
}: {
  market: MarketPrices;
  name: string;
  variantTitles: Record<string, string | null>;
  isWhyShown: boolean;
}) {
  const currencies = useCurrencies();
  return (
    <VStack gap={2}>
      <HStack gap={2} vAlign="center" wrap="wrap">
        <Link href={`/markets/${market.market}`} weight="semibold">
          {name}
        </Link>
        <Text type="supporting" color="secondary">
          {market.currency} ·{" "}
          {market.prices_include_tax ? "prices include tax" : "prices exclude tax"}
        </Text>
      </HStack>
      <Table>
        <TableHeader>
          <TableRow>
            <TableHeaderCell>Variant</TableHeaderCell>
            <TableHeaderCell>Price</TableHeaderCell>
            <TableHeaderCell>Set by</TableHeaderCell>
            <TableHeaderCell>Quantity breaks</TableHeaderCell>
          </TableRow>
        </TableHeader>
        <TableBody>
          {market.variants.map((v) => {
            const winner = v.candidates.find((c) => c.price_list === v.price_list);
            return (
              <TableRow key={v.variant}>
                <TableCell>{variantTitles[v.variant] ?? "Default"}</TableCell>
                <TableCell>
                  {v.problem ? (
                    <Text color="secondary">{v.problem.message}</Text>
                  ) : v.price ? (
                    <HStack gap={2} vAlign="center" wrap="wrap">
                      <Text weight="medium">{formatMoney(v.price, currencies)}</Text>
                      {v.compare_at_price && (
                        <Text color="secondary">
                          <s>{formatMoney(v.compare_at_price, currencies)}</s>
                        </Text>
                      )}
                      {v.on_sale && <Badge label="On sale" variant="pink" />}
                    </HStack>
                  ) : (
                    <Text color="secondary">—</Text>
                  )}
                </TableCell>
                <TableCell>
                  <VStack gap={1}>
                    {v.price_list ? (
                      <Link href={`/markets/price-lists/${v.price_list}`}>
                        {winner?.name ?? v.price_list}
                      </Link>
                    ) : (
                      <Text color="secondary">Base price</Text>
                    )}
                    {isWhyShown &&
                      v.candidates
                        .filter((c) => c.price_list !== v.price_list)
                        .map((c) => (
                          <Text key={c.price_list} type="supporting" color="secondary">
                            <Link href={`/markets/price-lists/${c.price_list}`}>{c.name}</Link>
                            {c.price ? ` (${formatMoney(c.price, currencies)})` : ""}:{" "}
                            {candidateReason(c.result, c.phase)}
                          </Text>
                        ))}
                    {isWhyShown && v.candidates.length === 0 && (
                      <Text type="supporting" color="secondary">
                        The market has no price lists.
                      </Text>
                    )}
                  </VStack>
                </TableCell>
                <TableCell>
                  {v.price_breaks.length === 0 ? (
                    <Text color="secondary">—</Text>
                  ) : (
                    <VStack gap={0}>
                      {v.price_breaks.map((b) => (
                        <Text key={b.min_quantity} type="supporting">
                          {formatMoney(b.price, currencies)} each from {b.min_quantity}
                        </Text>
                      ))}
                    </VStack>
                  )}
                </TableCell>
              </TableRow>
            );
          })}
        </TableBody>
      </Table>
    </VStack>
  );
}
