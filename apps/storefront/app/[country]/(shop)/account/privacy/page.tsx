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
import { loadAccount } from "@/lib/account";
import { countryOf } from "@/lib/api";
import { formatDate } from "@/lib/format";
import { pageT } from "@/lib/i18n";
import { link } from "@/lib/site";

export const dynamic = "force-dynamic";

export async function generateMetadata({
  params,
}: {
  params: Promise<{ country: string }>;
}): Promise<Metadata> {
  const t = await pageT((await params).country);
  return { title: t("account.privacy_meta_title") };
}

/** The person's data: download it, or ask for it to be erased. */
export default async function PrivacyPage({ params }: { params: Promise<{ country: string }> }) {
  const folder = (await params).country;
  const [country, t] = await Promise.all([countryOf(folder), pageT(folder)]);
  const code = country.country;
  const to = (path: string) => link(code, path);
  const account = await loadAccount();
  if (!account) redirect(to("/account"));
  return (
    <AccountShell t={t} title={t("account.privacy")} current="/account/privacy" link={to}>
      <Grid columns={{ minWidth: 320, repeat: "fit" }} gap={6}>
        <Card padding={5}>
          <VStack gap={3}>
            <Heading level={2}>{t("account.your_data")}</Heading>
            <Text color="secondary">{t("account.your_data_description")}</Text>
            <div>
              <Button
                label={t("account.download_data")}
                variant="secondary"
                href={to("/account/export")}
              />
            </div>
          </VStack>
        </Card>
        <Card padding={5}>
          <VStack gap={3}>
            <Heading level={2}>{t("account.erase_title")}</Heading>
            <ErasureControls
              country={code}
              dueAt={account.erasure_due_at ? formatDate(account.erasure_due_at, t.locale) : null}
            />
          </VStack>
        </Card>
      </Grid>
    </AccountShell>
  );
}
