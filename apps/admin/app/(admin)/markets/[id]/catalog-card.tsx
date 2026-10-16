"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { CheckboxInput } from "@astryxdesign/core/CheckboxInput";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { IconButton } from "@astryxdesign/core/IconButton";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
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
import type { AdminSchemas } from "@hypha/sdk/admin";
import { TrashIcon } from "@heroicons/react/24/outline";
import { useState, useTransition } from "react";

import { Pager } from "@/components/pager";
import { ProductStatusBadge } from "@/components/status";

import {
  type ProductChoice,
  addCatalogProducts,
  createCatalog,
  removeCatalogProduct,
  searchProducts,
} from "../actions";

type Market = AdminSchemas["MarketResponse"];
type Catalog = AdminSchemas["CatalogResponse"];
type Page = AdminSchemas["CatalogProductPage"];

/** What the market sells: its catalog's products, newest first. */
export function CatalogCard({
  market,
  catalog,
  page,
  isFirstPage,
  pricesCursor,
}: {
  market: Market;
  catalog?: Catalog;
  page?: Page;
  isFirstPage: boolean;
  /** The other list's cursor, kept when this one pages. */
  pricesCursor?: string;
}) {
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
              Catalog
            </Text>
            <Text type="supporting" color="secondary">
              The products this market can buy. A product also needs to be active and published
              to the storefront's channel.
            </Text>
          </VStack>
          {catalog ? (
            <Button label="Add products" variant="secondary" onClick={() => setIsAdding(true)} />
          ) : (
            <Button
              label="Create catalog"
              variant="primary"
              isLoading={isPending}
              onClick={() => run(() => createCatalog(market.id))}
            />
          )}
        </HStack>
        {error && <Banner status="error" title={error} />}
        {!catalog ? (
          <Text color="secondary">This market has no catalog yet, so it sells nothing.</Text>
        ) : !page || page.items.length === 0 ? (
          <Text color="secondary">
            {isFirstPage ? "No products yet." : "No more products."}
          </Text>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHeaderCell>Product</TableHeaderCell>
                <TableHeaderCell>Status</TableHeaderCell>
                <TableHeaderCell>
                  <span />
                </TableHeaderCell>
              </TableRow>
            </TableHeader>
            <TableBody>
              {page.items.map((product) => (
                <TableRow key={product.id}>
                  <TableCell>
                    {product.title ? (
                      <VStack gap={0}>
                        <Link href={`/products/${product.id}`} weight="medium">
                          {product.title}
                        </Link>
                        <Text type="supporting" color="secondary">
                          /{product.handle}
                        </Text>
                      </VStack>
                    ) : (
                      // An ID stored as given that names no product.
                      <Text color="secondary">{product.id} (no such product)</Text>
                    )}
                  </TableCell>
                  <TableCell>
                    {product.status && <ProductStatusBadge status={product.status} />}
                  </TableCell>
                  <TableCell>
                    <HStack hAlign="end">
                      <IconButton
                        label={`Remove ${product.title ?? product.id}`}
                        icon={<TrashIcon />}
                        size="sm"
                        variant="ghost"
                        isDisabled={isPending}
                        onClick={() =>
                          run(() => removeCatalogProduct(market.id, catalog.id, product.id))
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
            query={{ prices: pricesCursor }}
            param="products"
            isFirstPage={isFirstPage}
            nextCursor={page.next_cursor}
          />
        )}
      </VStack>
      {isAdding && catalog && (
        <AddProductsDialog
          onAdd={(ids) => addCatalogProducts(market.id, catalog.id, ids)}
          onClose={() => setIsAdding(false)}
        />
      )}
    </Card>
  );
}

/** Search products by title, handle or SKU, tick some, add them. */
function AddProductsDialog({
  onAdd,
  onClose,
}: {
  onAdd: (ids: string[]) => Promise<{ error?: string }>;
  onClose: () => void;
}) {
  const [q, setQ] = useState("");
  const [found, setFound] = useState<ProductChoice[]>();
  const [picked, setPicked] = useState<string[]>([]);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();

  const search = () => startTransition(async () => setFound(await searchProducts(q)));
  const add = () =>
    startTransition(async () => {
      const result = await onAdd(picked);
      if (result.error) setError(result.error);
      else onClose();
    });

  return (
    <Dialog isOpen onOpenChange={(isOpen) => !isOpen && onClose()} width={560} purpose="form">
      <DialogHeader title="Add products" onOpenChange={(isOpen) => !isOpen && onClose()} />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <HStack gap={2} vAlign="end">
          <TextInput
            label="Search"
            placeholder="Title, handle or SKU"
            value={q}
            onChange={setQ}
            onEnter={search}
            hasAutoFocus
          />
          <Button label="Search" onClick={search} isLoading={isPending && !found} />
        </HStack>
        {found && found.length === 0 && <Text color="secondary">No products match.</Text>}
        {found?.map((product) => (
          <CheckboxInput
            key={product.id}
            label={product.title}
            description={`/${product.handle}`}
            value={picked.includes(product.id)}
            onChange={(checked) =>
              setPicked(checked ? [...picked, product.id] : picked.filter((id) => id !== product.id))
            }
          />
        ))}
        <HStack gap={2} hAlign="end">
          <Button label="Cancel" variant="ghost" onClick={onClose} />
          <Button
            label={picked.length > 1 ? `Add ${picked.length} products` : "Add product"}
            variant="primary"
            isDisabled={picked.length === 0}
            isLoading={isPending && found !== undefined}
            onClick={add}
          />
        </HStack>
      </VStack>
    </Dialog>
  );
}
