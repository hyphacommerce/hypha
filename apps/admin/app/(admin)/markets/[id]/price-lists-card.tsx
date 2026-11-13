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
import { useFormat } from "@/lib/use-format";
import { useT } from "@/lib/use-t";

import { addToCatalog, removeFromCatalog } from "../price-lists/actions";

type Market = AdminSchemas["MarketResponse"];
type Catalog = AdminSchemas["CatalogResponse"];

/** The scope reading price lists takes: a name, shown as it is. */
const READ_SCOPE = "pricing:read";

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
  const t = useT<"markets" | "price_lists">();
  const format = useFormat();
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
              {t("markets.price_lists_title")}
            </Text>
            <Text type="supporting" color="secondary">
              {catalog.price_lists.length === 0
                ? t("markets.price_lists_description_none", { currency: market.currency })
                : t("markets.price_lists_description")}
            </Text>
          </VStack>
          {canCreate && (
            <Button
              label={t("markets.price_lists_new")}
              variant="secondary"
              href={`/markets/price-lists/new?market=${market.id}`}
            />
          )}
        </HStack>
        {error && <Banner status="error" title={error} />}
        {!canRead ? (
          <Text color="secondary">{t("markets.price_lists_no_scope", { scope: READ_SCOPE })}</Text>
        ) : (
          <>
            {lists.length > 0 && (
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHeaderCell>{t("markets.price_lists_column_list")}</TableHeaderCell>
                    <TableHeaderCell>{t("common.kind")}</TableHeaderCell>
                    <TableHeaderCell>{t("common.status")}</TableHeaderCell>
                    <TableHeaderCell>
                      {t("markets.price_lists_column_fixed_prices")}
                    </TableHeaderCell>
                    <TableHeaderCell>
                      <span className="visually-hidden">{t("common.remove")}</span>
                    </TableHeaderCell>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {lists.map((list) => {
                    const phase = phaseBadge(list.phase, t);
                    return (
                      <TableRow key={list.id}>
                        <TableCell>
                          <VStack gap={0}>
                            <Link href={`/markets/price-lists/${list.id}`} weight="medium">
                              {list.name}
                            </Link>
                            <Text type="supporting" color="secondary">
                              {adjustmentLabel(t, format, list.adjustment)}
                            </Text>
                          </VStack>
                        </TableCell>
                        <TableCell>{kindLabel(t, list.kind)}</TableCell>
                        <TableCell>
                          <Badge label={phase.label} variant={phase.variant} />
                        </TableCell>
                        <TableCell>{format.number(list.fixed_price_count)}</TableCell>
                        <TableCell>
                          {canWrite && (
                            <HStack hAlign="end">
                              <IconButton
                                label={t("markets.price_lists_remove", {
                                  list: list.name,
                                  market: market.name,
                                })}
                                tooltip={t("markets.price_lists_remove_tooltip")}
                                icon={<XMarkIcon />}
                                size="sm"
                                variant="ghost"
                                isDisabled={isPending}
                                onClick={() => run(() => removeFromCatalog(list.id, catalog.id))}
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
                  label={t("markets.price_lists_add_label", { currency: market.currency })}
                  options={offered.map((l) => ({
                    value: l.id,
                    label: l.name,
                    description: t("markets.price_lists_option_description", {
                      kind: kindLabel(t, l.kind),
                      adjustment: adjustmentLabel(t, format, l.adjustment),
                    }),
                  }))}
                  value={adding}
                  onChange={setAdding}
                  hasSearch={offered.length > 8}
                  placeholder={t("markets.price_lists_add_placeholder")}
                  width={280}
                />
                <Button
                  label={t("common.add")}
                  isDisabled={!adding || isPending}
                  onClick={() => adding && run(() => addToCatalog(adding, catalog.id))}
                />
              </HStack>
            )}
            {isFull && (
              <Text type="supporting" color="secondary">
                {t("markets.price_lists_full", { max: MAX_LISTS_PER_CATALOG })}
              </Text>
            )}
          </>
        )}
      </VStack>
    </Card>
  );
}
