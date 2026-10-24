import { Button } from "@astryxdesign/core/Button";
import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Suspense } from "react";

import { PageHeader } from "@/components/page-header";
import { type SearchParams, one, oneOf } from "@/components/resource-table/params";
import { listPage, pageQuery } from "@/components/resource-table/server";
import { requireStaff, sessionClient } from "@/lib/hypha";
import { hasScope } from "@/lib/navigation";

import { DiscountsTable } from "./discounts-table";

const PHASES = ["draft", "scheduled", "active", "expired"] as const;
const METHODS = ["automatic", "codes"] as const;
const CLASSES = ["product", "order", "shipping"] as const;

/**
 * Every discount: how buyers get it, what it gives, where it stands, when and how often
 * it was used. The header and the table's toolbar render at once; the rows stream in.
 */
export default async function DiscountsPage({
  searchParams,
}: {
  searchParams: Promise<SearchParams>;
}) {
  const [params, me] = await Promise.all([searchParams, requireStaff()]);
  const canWrite = hasScope(me.scopes, "discounts:write");
  return (
    <Section padding={6}>
      <VStack gap={5}>
        <PageHeader
          title="Discounts"
          description="Codes buyers enter and discounts that apply by themselves: off products, the order or shipping, buy X get Y and volume tiers."
          actions={
            canWrite && <Button label="Create discount" variant="primary" href="/discounts/new" />
          }
        />
        <Suspense fallback={<DiscountsTable canWrite={canWrite} />}>
          <DiscountRows params={params} canWrite={canWrite} />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function DiscountRows({ params, canWrite }: { params: SearchParams; canWrite: boolean }) {
  const client = await sessionClient();
  const result = await client.GET("/admin/discounts", {
    params: {
      query: {
        ...pageQuery(params),
        q: one(params, "q")?.trim() || undefined,
        status: oneOf(params, "status", PHASES),
        method: oneOf(params, "method", METHODS),
        class: oneOf(params, "class", CLASSES),
      },
    },
  });
  return <DiscountsTable page={listPage(result, "/discounts", params)} canWrite={canWrite} />;
}
