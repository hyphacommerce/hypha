import { Badge } from "@astryxdesign/core/Badge";
import { Button } from "@astryxdesign/core/Button";
import { EmptyState } from "@astryxdesign/core/EmptyState";
import { VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
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
import { GlobeEuropeAfricaIcon } from "@heroicons/react/24/outline";

import { PageHeader } from "@/components/page-header";
import { loadCountries, loaded, sessionClient } from "@/lib/hypha";
import { countryLabel } from "@/lib/regions";

/** Markets (design note 0021, section 8): who the store sells to, in which currency. */
export default async function MarketsPage() {
  const client = await sessionClient();
  const [markets, countries] = await Promise.all([
    client.GET("/admin/markets").then(loaded),
    loadCountries(),
  ]);

  return (
    <Section padding={6}>
      <VStack gap={5}>
        <PageHeader
          title="Markets"
          description="Countries that buy in one currency, with the catalog of what they can buy and its prices."
          actions={<Button label="Add market" variant="primary" href="/markets/new" />}
        />
        {markets.items.length === 0 ? (
          <EmptyState
            icon={<GlobeEuropeAfricaIcon width={32} />}
            title="No markets yet"
            description="A storefront sells only to countries in a market. Add one to start selling."
            actions={<Button label="Add market" href="/markets/new" />}
          />
        ) : (
          <Table hasHover>
            <TableHeader>
              <TableRow>
                <TableHeaderCell>Market</TableHeaderCell>
                <TableHeaderCell>Countries</TableHeaderCell>
                <TableHeaderCell>Currency</TableHeaderCell>
                <TableHeaderCell>Prices</TableHeaderCell>
                <TableHeaderCell>Catalog</TableHeaderCell>
              </TableRow>
            </TableHeader>
            <TableBody>
              {markets.items.map((market) => (
                <TableRow key={market.id}>
                  <TableCell>
                    <Link href={`/markets/${market.id}`} weight="medium">
                      {market.name}
                    </Link>
                  </TableCell>
                  <TableCell>
                    <Text>{market.countries.map((c) => countryLabel(c, countries)).join(", ")}</Text>
                  </TableCell>
                  <TableCell>{market.currency}</TableCell>
                  <TableCell>
                    <Text color="secondary">
                      {market.prices_include_tax ? "Include tax" : "Exclude tax"}
                    </Text>
                  </TableCell>
                  <TableCell>
                    {market.catalog ? (
                      <Badge label="Ready" variant="success" />
                    ) : (
                      <Badge label="None yet" variant="warning" />
                    )}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}
      </VStack>
    </Section>
  );
}
