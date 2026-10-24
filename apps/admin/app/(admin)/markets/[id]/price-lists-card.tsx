"use client";

import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
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
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { XMarkIcon } from "@heroicons/react/24/outline";
import { useState, useTransition } from "react";

import {
  MAX_LISTS_PER_CATALOG,
  type PriceList,
  adjustmentLabel,
  kindLabel,
  phaseBadge,
} from "@/lib/price-lists";

import { addToCatalog, removeFromCatalog } from "../price-lists/actions";

type Market = AdminSchemas["MarketResponse"];
type Catalog = AdminSchemas["CatalogResponse"];

/**
 * The market's price lists: every list its catalog holds, added and removed here. The
 * lowest live price among them is what shoppers pay; each list is edited on its page.
 */
export function PriceListsCard({
  market,
  catalog,
  canRead,
  canWrite,
  canCreate,
  lists,
  offered,
}: {
  market: Market;
  catalog: Catalog;
  /** Whether the user may read price lists at all (`pricing:read`). */
  canRead: boolean;
  /** `markets:write`: changing which lists the catalog holds. */
  canWrite: boolean;
  /** `pricing:write`: making a new list. */
  canCreate: boolean;
  /** The catalog's lists, in its order. */
  lists: PriceList[];
  /** Lists in the market's currency the catalog doesn't hold, to add. */
  offered: PriceList[];
}) {
  const [adding, setAdding] = useState<string>();
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const run = (action: () => Promise<{ error?: string }>) =>
    startTransition(async () => {
      const result = await action();
      setError(result.error);
      if (!result.error) setAdding(undefined);
    });
  const isFull = catalog.price_lists.length >= MAX_LISTS_PER_CATALOG;

  return (
    <Card>
      <VStack gap={3}>
        <HStack gap={2} hAlign="between" vAlign="center" wrap="wrap">
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              Price lists
            </Text>
            <Text type="supporting" color="secondary">
              {catalog.price_lists.length === 0
                ? `Without one, the market sells at each variant's base price (when it's in ${market.currency}).`
                : "Shoppers pay the lowest live price among these lists, or the base price where none prices a variant. Put lists together only when they should compete: a markup beside a regular list never wins."}
            </Text>
          </VStack>
          {canCreate && (
            <Button
              label="New price list"
              variant="secondary"
              href={`/markets/price-lists/new?market=${market.id}`}
            />
          )}
        </HStack>
        {error && <Banner status="error" title={error} />}
        {!canRead ? (
          <Text color="secondary">Viewing price lists needs the pricing:read scope.</Text>
        ) : (
          <>
            {lists.length > 0 && (
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHeaderCell>List</TableHeaderCell>
                    <TableHeaderCell>Kind</TableHeaderCell>
                    <TableHeaderCell>Status</TableHeaderCell>
                    <TableHeaderCell>Fixed prices</TableHeaderCell>
                    <TableHeaderCell>
                      <span className="visually-hidden">Remove</span>
                    </TableHeaderCell>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {lists.map((list) => {
                    const phase = phaseBadge(list.phase);
                    return (
                      <TableRow key={list.id}>
                        <TableCell>
                          <VStack gap={0}>
                            <Link href={`/markets/price-lists/${list.id}`} weight="medium">
                              {list.name}
                            </Link>
                            <Text type="supporting" color="secondary">
                              {adjustmentLabel(list.adjustment)}
                            </Text>
                          </VStack>
                        </TableCell>
                        <TableCell>{kindLabel(list.kind)}</TableCell>
                        <TableCell>
                          <Badge label={phase.label} variant={phase.variant} />
                        </TableCell>
                        <TableCell>{list.fixed_price_count.toLocaleString("en")}</TableCell>
                        <TableCell>
                          {canWrite && (
                            <HStack hAlign="end">
                              <IconButton
                                label={`Remove ${list.name} from ${market.name}`}
                                tooltip="Remove from this market (the list stays)"
                                icon={<XMarkIcon />}
                                size="sm"
                                variant="ghost"
                                isDisabled={isPending}
                                onClick={() =>
                                  run(() => removeFromCatalog(list.id, catalog.id))
                                }
                              />
                            </HStack>
                          )}
                        </TableCell>
                      </TableRow>
                    );
                  })}
                </TableBody>
              </Table>
            )}
            {canWrite && offered.length > 0 && !isFull && (
              <HStack gap={2} vAlign="end" wrap="wrap">
                <Selector
                  label={`Add a ${market.currency} price list`}
                  options={offered.map((l) => ({
                    value: l.id,
                    label: l.name,
                    description: `${kindLabel(l.kind)} · ${adjustmentLabel(l.adjustment)}`,
                  }))}
                  value={adding}
                  onChange={setAdding}
                  hasSearch={offered.length > 8}
                  placeholder="Choose a price list"
                  width={280}
                />
                <Button
                  label="Add"
                  isDisabled={!adding || isPending}
                  onClick={() =>
                    adding && run(() => addToCatalog(adding, catalog.id))
                  }
                />
              </HStack>
            )}
            {isFull && (
              <Text type="supporting" color="secondary">
                A catalog holds at most {MAX_LISTS_PER_CATALOG} price lists.
              </Text>
            )}
          </>
        )}
      </VStack>
    </Card>
  );
}
