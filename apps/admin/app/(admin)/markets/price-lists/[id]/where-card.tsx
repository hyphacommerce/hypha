"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { IconButton } from "@astryxdesign/core/IconButton";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { Selector } from "@astryxdesign/core/Selector";
import { Text } from "@astryxdesign/core/Text";
import { XMarkIcon } from "@heroicons/react/24/outline";
import { useState, useTransition } from "react";

import type { PriceList } from "@/lib/price-lists";
import { useT } from "@/lib/use-t";

import { addToCatalog, removeFromCatalog } from "../actions";
import type { MarketOption } from "../price-list-form";

/**
 * Where the list prices: the markets whose catalogs use it, added and removed here. Only
 * markets in the list's currency can use it.
 */
export function WhereCard({
  list,
  markets,
  canWrite,
}: {
  list: PriceList;
  /** Every market with a catalog. */
  markets: MarketOption[];
  /** `markets:write`: attaching a list changes the market's catalog. */
  canWrite: boolean;
}) {
  const t = useT<"price_lists">();
  const [adding, setAdding] = useState<string>();
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const using = list.markets.map(
    (id) =>
      markets.find((m) => m.id === id) ?? { id, name: id, currency: list.currency, catalog: "" },
  );
  const offered = markets.filter(
    (m) => m.currency === list.currency && !list.markets.includes(m.id),
  );
  const run = (action: () => Promise<{ error?: string }>) =>
    startTransition(async () => {
      const result = await action();
      setError(result.error);
      if (!result.error) setAdding(undefined);
    });

  return (
    <Card>
      <VStack gap={3}>
        <VStack gap={1}>
          <Text type="display-3" as="h2">
            {t("common.markets")}
          </Text>
          <Text type="supporting" color="secondary">
            {t("price_lists.where_description")}
          </Text>
        </VStack>
        {error && <Banner status="error" title={error} />}
        {using.length === 0 ? (
          <Text color="secondary">{t("price_lists.where_none")}</Text>
        ) : (
          <VStack gap={1}>
            {using.map((m) => (
              <HStack key={m.id} gap={2} hAlign="between" vAlign="center">
                <Link href={`/markets/${m.id}`} weight="medium">
                  {m.name}
                </Link>
                {canWrite && m.catalog && (
                  <IconButton
                    label={t("price_lists.where_remove", { market: m.name })}
                    icon={<XMarkIcon />}
                    size="sm"
                    variant="ghost"
                    isDisabled={isPending}
                    onClick={() => run(() => removeFromCatalog(list.id, m.catalog))}
                  />
                )}
              </HStack>
            ))}
          </VStack>
        )}
        {canWrite && offered.length > 0 && (
          <HStack gap={2} vAlign="end" wrap="wrap">
            <Selector
              label={t("price_lists.where_add_label", { currency: list.currency })}
              options={offered.map((m) => ({ value: m.id, label: m.name }))}
              value={adding}
              onChange={setAdding}
              placeholder={t("price_lists.where_add_placeholder")}
              width={200}
            />
            <Button
              label={t("common.add")}
              isDisabled={!adding || isPending}
              isLoading={isPending && Boolean(adding)}
              onClick={() => {
                const market = offered.find((m) => m.id === adding);
                if (market) run(() => addToCatalog(list.id, market.catalog));
              }}
            />
          </HStack>
        )}
        {canWrite && offered.length === 0 && using.length === 0 && (
          <Text type="supporting" color="secondary">
            {t("price_lists.where_no_market", { currency: list.currency })}
          </Text>
        )}
      </VStack>
    </Card>
  );
}
