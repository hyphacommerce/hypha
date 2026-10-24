"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { IconButton } from "@astryxdesign/core/IconButton";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { Selector } from "@astryxdesign/core/Selector";
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
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { TrashIcon } from "@heroicons/react/24/outline";
import { useState, useTransition } from "react";

import { useCurrencies } from "@/components/currencies";
import { Pager } from "@/components/pager";
import { type PickerProduct, ProductPicker } from "@/components/product-picker";
import { type PickerVariant, pickerVariants } from "@/components/product-picker-actions";
import { formatMoney } from "@/lib/format";

import {
  attachPriceList,
  detachPriceList,
  removeFixedPrice,
  setAdjustment,
  setFixedPrice,
} from "../actions";

type Market = AdminSchemas["MarketResponse"];
type Catalog = AdminSchemas["CatalogResponse"];
type PriceList = AdminSchemas["PriceListResponse"];
type Page = AdminSchemas["FixedPricePage"];

/** "-10" for -1000 basis points: what the percent input shows. */
function percentOf(bps: number): string {
  return String(bps / 100);
}

/**
 * Basis points from a typed percentage ("-10", "5.5"), or null when it isn't a number
 * with at most two decimals. Negative is a discount.
 */
function parsePercent(input: string): number | null {
  const match = /^(-)?(\d+)(?:[.,](\d{1,2}))?$/.exec(input.trim());
  if (!match) return null;
  const [, minus, whole, fraction = ""] = match;
  const bps = Number(whole) * 100 + Number(fraction.padEnd(2, "0"));
  return minus ? -bps : bps;
}

/** How the market's prices are worked out: an adjustment of the base prices, and fixed
 * prices for some variants. */
export function PriceListCard({
  market,
  catalog,
  canRead,
  lists,
  list,
  page,
  isFirstPage,
  productsCursor,
}: {
  market: Market;
  catalog: Catalog;
  /** Whether the user may read price lists at all (`pricing:read`). */
  canRead: boolean;
  /** The price lists in the market's currency, to pick from. */
  lists: PriceList[];
  list?: PriceList;
  page?: Page;
  isFirstPage: boolean;
  productsCursor?: string;
}) {
  const currencies = useCurrencies();
  const [error, setError] = useState<string>();
  const [isAdding, setIsAdding] = useState(false);
  const [isPending, startTransition] = useTransition();
  const run = (action: () => Promise<{ error?: string }>) =>
    startTransition(async () => setError((await action()).error));

  return (
    <Card>
      <VStack gap={3}>
        <HStack gap={2} hAlign="between" vAlign="center" wrap="wrap">
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              Prices
            </Text>
            <Text type="supporting" color="secondary">
              {list
                ? `Prices in ${market.currency}: each variant's fixed price, or its base price with the adjustment.`
                : `Without a price list, the market sells at each variant's base price (if it's in ${market.currency}).`}
            </Text>
          </VStack>
          {list && (
            <HStack gap={2}>
              <Button
                label="Add fixed price"
                variant="secondary"
                onClick={() => setIsAdding(true)}
              />
              <Button
                label="Detach"
                variant="ghost"
                isDisabled={isPending}
                onClick={() => run(() => detachPriceList(market.id, catalog.id))}
              />
            </HStack>
          )}
        </HStack>
        {error && <Banner status="error" title={error} />}
        {!canRead ? (
          <Text color="secondary">Viewing price lists needs the pricing:read scope.</Text>
        ) : !list ? (
          <AttachForm market={market} catalog={catalog} lists={lists} />
        ) : (
          <>
            <AdjustmentForm market={market} list={list} />
            {!page || page.items.length === 0 ? (
              <Text color="secondary">
                {isFirstPage
                  ? "No fixed prices: every variant follows the adjustment."
                  : "No more."}
              </Text>
            ) : (
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHeaderCell>Variant</TableHeaderCell>
                    <TableHeaderCell>Fixed price</TableHeaderCell>
                    <TableHeaderCell>
                      <span />
                    </TableHeaderCell>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {page.items.map((fixed) => (
                    <TableRow key={fixed.variant}>
                      <TableCell>
                        {fixed.product ? (
                          <VStack gap={0}>
                            <Link href={`/products/${fixed.product.id}`} weight="medium">
                              {fixed.product.title}
                            </Link>
                            {fixed.product.variant_title && (
                              <Text type="supporting" color="secondary">
                                {fixed.product.variant_title}
                              </Text>
                            )}
                          </VStack>
                        ) : (
                          <Text color="secondary">{fixed.variant} (no such variant)</Text>
                        )}
                      </TableCell>
                      <TableCell>{formatMoney(fixed.price, currencies)}</TableCell>
                      <TableCell>
                        <HStack hAlign="end">
                          <IconButton
                            label="Remove the fixed price"
                            icon={<TrashIcon />}
                            size="sm"
                            variant="ghost"
                            isDisabled={isPending}
                            onClick={() =>
                              run(() => removeFixedPrice(market.id, list.id, fixed.variant))
                            }
                          />
                        </HStack>
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            )}
            {page && (
              <Pager
                path={`/markets/${market.id}`}
                query={{ products: productsCursor }}
                param="prices"
                isFirstPage={isFirstPage}
                nextCursor={page.next_cursor}
              />
            )}
          </>
        )}
      </VStack>
      {isAdding && list && (
        <FixedPriceDialog
          currency={market.currency}
          onSave={(variant, price) =>
            setFixedPrice(market.id, list.id, { variant, price, currency: market.currency })
          }
          onClose={() => setIsAdding(false)}
        />
      )}
    </Card>
  );
}

/** Gives the catalog a price list: a new one with an adjustment, or an existing one. */
function AttachForm({
  market,
  catalog,
  lists,
}: {
  market: Market;
  catalog: Catalog;
  lists: PriceList[];
}) {
  const [percent, setPercent] = useState("0");
  const [existing, setExisting] = useState<string>();
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const bps = parsePercent(percent);

  const attach = (input: Parameters<typeof attachPriceList>[2]) =>
    startTransition(async () =>
      setError((await attachPriceList(market.id, catalog.id, input)).error),
    );

  return (
    <VStack gap={3}>
      {error && <Banner status="error" title={error} />}
      <HStack gap={2} vAlign="end" wrap="wrap">
        <TextInput
          label="Adjustment, %"
          description="Applied to every base price: -10 is 10% off, 5 is 5% more."
          value={percent}
          onChange={setPercent}
          status={
            bps === null ? { type: "error", message: "A number, like -10 or 5.5." } : undefined
          }
        />
        <Button
          label="Create price list"
          variant="primary"
          isLoading={isPending}
          isDisabled={bps === null}
          onClick={() =>
            bps !== null &&
            attach({
              name: `${market.name} prices`,
              currency: market.currency,
              adjustmentBps: bps,
            })
          }
        />
      </HStack>
      {lists.length > 0 && (
        <HStack gap={2} vAlign="end" wrap="wrap">
          <Selector
            label={`Or use an existing ${market.currency} list`}
            options={lists.map((l) => ({
              value: l.id,
              label: `${l.name} · ${percentOf(l.adjustment?.percent_bps ?? 0)}%`,
            }))}
            value={existing}
            onChange={setExisting}
            placeholder="Choose a price list"
          />
          <Button
            label="Use it"
            isDisabled={!existing || isPending}
            onClick={() => existing && attach({ existing })}
          />
        </HStack>
      )}
    </VStack>
  );
}

function AdjustmentForm({ market, list }: { market: Market; list: PriceList }) {
  const [percent, setPercent] = useState(() => percentOf(list.adjustment?.percent_bps ?? 0));
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const bps = parsePercent(percent);
  const isChanged = bps !== null && bps !== (list.adjustment?.percent_bps ?? 0);

  return (
    <VStack gap={2}>
      {error && <Banner status="error" title={error} />}
      <HStack gap={2} vAlign="end">
        <TextInput
          label="Adjustment, %"
          description="Applied to every base price without a fixed price: -10 is 10% off."
          value={percent}
          onChange={setPercent}
          status={
            bps === null ? { type: "error", message: "A number, like -10 or 5.5." } : undefined
          }
        />
        <Button
          label="Save"
          isLoading={isPending}
          isDisabled={!isChanged}
          onClick={() =>
            bps !== null &&
            startTransition(async () =>
              setError((await setAdjustment(market.id, list.id, bps)).error),
            )
          }
        />
      </HStack>
    </VStack>
  );
}

/**
 * Pick a product (the shared picker), then its variant and the price. A product with one
 * variant skips straight to the price.
 */
function FixedPriceDialog({
  currency,
  onSave,
  onClose,
}: {
  currency: string;
  onSave: (variant: string, price: string) => Promise<{ error?: string }>;
  onClose: () => void;
}) {
  const currencies = useCurrencies();
  const [product, setProduct] = useState<PickerProduct>();
  const [variants, setVariants] = useState<PickerVariant[]>();
  const [variant, setVariant] = useState<string>();
  const [price, setPrice] = useState("");
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();

  if (!product) {
    return (
      <ProductPicker
        title="Add fixed price"
        description="First the product, then its variant and the price."
        mode="single"
        confirmLabel={() => "Next"}
        onConfirm={async ([picked]) => {
          if (!picked) return;
          const found = await pickerVariants(picked.id);
          setVariants(found);
          setVariant(found.length === 1 ? found[0]?.id : undefined);
          setProduct(picked);
        }}
        onClose={onClose}
      />
    );
  }

  const save = () =>
    variant &&
    startTransition(async () => {
      const result = await onSave(variant, price);
      if (result.error) setError(result.error);
      else onClose();
    });
  const options = (variants ?? []).map((v) => ({
    value: v.id,
    label: v.title ?? product.title,
    description: [v.sku && `SKU ${v.sku}`, `Base price ${formatMoney(v.price, currencies)}`]
      .filter(Boolean)
      .join(" · "),
  }));

  return (
    <Dialog isOpen onOpenChange={(isOpen) => !isOpen && onClose()} width={520} purpose="form">
      <DialogHeader
        title={`Fixed price for ${product.title}`}
        subtitle="The variant costs exactly this in the market, whatever the adjustment."
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        {options.length > 1 && (
          <Selector
            label="Variant"
            options={options}
            value={variant}
            onChange={setVariant}
            hasSearch
            placeholder="Choose a variant"
          />
        )}
        {options.length === 1 && (
          <Text type="supporting" color="secondary">
            {options[0]?.description}
          </Text>
        )}
        <TextInput
          label={`Price, ${currency}`}
          placeholder="18.00"
          value={price}
          onChange={setPrice}
          onEnter={save}
          hasAutoFocus
        />
        <HStack gap={2} hAlign="between">
          <Button label="Back" variant="ghost" onClick={() => setProduct(undefined)} />
          <HStack gap={2}>
            <Button label="Cancel" variant="ghost" onClick={onClose} />
            <Button
              label="Save"
              variant="primary"
              isDisabled={!variant || !price.trim()}
              isLoading={isPending}
              onClick={save}
            />
          </HStack>
        </HStack>
      </VStack>
    </Dialog>
  );
}
