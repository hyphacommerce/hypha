import { Button } from "@astryxdesign/core/Button";
import { EmptyState } from "@astryxdesign/core/EmptyState";
import { Heading } from "@astryxdesign/core/Heading";
import { VStack } from "@astryxdesign/core/Stack";
import type { Metadata } from "next";

import { countryOf } from "@/lib/api";
import { link } from "@/lib/site";

/** Per request, like the cart: accounts will be the shopper's own (design note 0024). */
export const dynamic = "force-dynamic";

export const metadata: Metadata = { title: "Account" };

/** Customer accounts aren't built yet (design note 0008, §9): checkout works as a guest. */
export default async function AccountPage({ params }: { params: Promise<{ country: string }> }) {
  const country = await countryOf((await params).country);
  return (
    <VStack gap={6} maxWidth={640}>
      <Heading level={1} type="display-2">
        Account
      </Heading>
      <EmptyState
        title="Accounts are coming"
        description="For now you check out as a guest, with your email and address."
        actions={<Button label="Keep shopping" href={link(country.country, "/")} variant="primary" />}
      />
    </VStack>
  );
}
