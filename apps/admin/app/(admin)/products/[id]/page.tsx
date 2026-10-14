import { Badge } from "@astryxdesign/core/Badge";
import { Card } from "@astryxdesign/core/Card";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { MetadataList, MetadataListItem } from "@astryxdesign/core/MetadataList";
import { Section } from "@astryxdesign/core/Section";
import {
  Table,
  TableBody,
  TableCell,
  TableHeader,
  TableHeaderCell,
  TableRow,
} from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";

import { PageHeader } from "@/components/page-header";
import { ProductStatusBadge } from "@/components/status";
import { formatMoney } from "@/lib/format";
import { loaded, sessionClient } from "@/lib/hypha";

import { StatusActions } from "./status-actions";

export default async function ProductPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  const client = await sessionClient();
  const product = loaded(
    await client.GET("/admin/products/{id}", { params: { path: { id } } }),
  );
  const hasOptions = product.options.length > 0;

  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={960}>
        <PageHeader
          title={product.title}
          back={{ label: "Products", href: "/products" }}
          badges={<ProductStatusBadge status={product.status} />}
          actions={<StatusActions id={product.id} status={product.status} />}
        />

        <Card>
          <MetadataList title="Details">
            <MetadataListItem label="Handle">/{product.handle}</MetadataListItem>
            <MetadataListItem label="ID">
              <Text type="supporting" color="secondary">
                {product.id}
              </Text>
            </MetadataListItem>
          </MetadataList>
        </Card>

        {hasOptions && (
          <Card>
            <VStack gap={3}>
              <Text type="display-3" as="h2">
                Options
              </Text>
              {product.options.map((option) => (
                <HStack key={option.id} gap={2} vAlign="center" wrap="wrap">
                  <Text type="label">{option.name}</Text>
                  {option.values.map((value) => (
                    <Badge key={value.id} label={value.label} />
                  ))}
                </HStack>
              ))}
            </VStack>
          </Card>
        )}

        <Card>
          <VStack gap={3}>
            <VStack gap={1}>
              <Text type="display-3" as="h2">
                Variants
              </Text>
              <Text type="supporting" color="secondary">
                Base prices, before markets and price lists adjust them.
              </Text>
            </VStack>
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHeaderCell>Variant</TableHeaderCell>
                  <TableHeaderCell>SKU</TableHeaderCell>
                  <TableHeaderCell>Price</TableHeaderCell>
                </TableRow>
              </TableHeader>
              <TableBody>
                {product.variants.map((variant) => (
                  <TableRow key={variant.id}>
                    <TableCell>{variant.title ?? (hasOptions ? "—" : "Default")}</TableCell>
                    <TableCell>{variant.sku ?? "—"}</TableCell>
                    <TableCell>{formatMoney(variant.price)}</TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </VStack>
        </Card>
      </VStack>
    </Section>
  );
}
