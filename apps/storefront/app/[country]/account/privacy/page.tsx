import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { Grid } from "@astryxdesign/core/Grid";
import { Heading } from "@astryxdesign/core/Heading";
import { VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import type { Metadata } from "next";
import { redirect } from "next/navigation";

import { AccountShell } from "@/components/account/account-shell";
import { ErasureControls } from "@/components/account/forms";
import { PLACED_DATE } from "@/components/order-details";
import { loadAccount } from "@/lib/account";
import { countryOf } from "@/lib/api";
import { link } from "@/lib/site";

export const dynamic = "force-dynamic";

export const metadata: Metadata = { title: "Privacy" };

/** The person's data: download it, or ask for it to be erased. */
export default async function PrivacyPage({ params }: { params: Promise<{ country: string }> }) {
  const country = await countryOf((await params).country);
  const code = country.country;
  const to = (path: string) => link(code, path);
  const account = await loadAccount();
  if (!account) redirect(to("/account"));
  return (
    <AccountShell title="Privacy" current="/account/privacy" link={to}>
      <Grid columns={{ minWidth: 320, repeat: "fit" }} gap={6}>
        <Card padding={5}>
          <VStack gap={3}>
            <Heading level={2}>Your data</Heading>
            <Text color="secondary">
              Everything we hold about you, as one file: your details, addresses, marketing choices,
              orders, payments and the emails we sent you.
            </Text>
            <div>
              <Button label="Download my data" variant="secondary" href={to("/account/export")} />
            </div>
          </VStack>
        </Card>
        <Card padding={5}>
          <VStack gap={3}>
            <Heading level={2}>Erase your data</Heading>
            <ErasureControls
              country={code}
              dueAt={
                account.erasure_due_at ? PLACED_DATE.format(new Date(account.erasure_due_at)) : null
              }
            />
          </VStack>
        </Card>
      </Grid>
    </AccountShell>
  );
}
