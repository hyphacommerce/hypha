import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Suspense } from "react";

import { type Crumb, PageHeader } from "@/components/page-header";
import { DetailSkeleton } from "@/components/skeletons";
import { TransferStatusBadge } from "@/components/status";
import { loaded, requireStaff, sessionClient } from "@/lib/hypha";
import { getT } from "@/lib/i18n";
import { hasScope } from "@/lib/navigation";
import { trNumber } from "@/lib/transfers";

import { TransferActions, TransferView } from "./transfer-view";

/**
 * A transfer: what moves from where to where, and how much of each line left and arrived.
 * A draft's lines and details change in place; the header's actions ship it, receive it,
 * cancel or delete it.
 */
export default async function TransferPage({ params }: { params: Promise<{ id: string }> }) {
  const [{ id }, t] = await Promise.all([params, getT()]);
  const back: Crumb[] = [{ label: t("transfers.list_title"), href: "/transfers" }];
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={1280}>
        <Suspense fallback={<DetailSkeleton back={back} />}>
          <Transfer id={id} back={back} />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function Transfer({ id, back }: { id: string; back: Crumb[] }) {
  const client = await sessionClient();
  const [transfer, me, locations, t] = await Promise.all([
    client.GET("/admin/transfers/{id}", { params: { path: { id } } }).then(loaded),
    requireStaff(),
    client.GET("/admin/locations"),
    getT(),
  ]);
  const canWrite = hasScope(me.scopes, "inventory:write");
  const places = (locations.data?.items ?? []).map((l) => ({ id: l.id, name: l.name }));
  const nameOf = (location: string) => places.find((p) => p.id === location)?.name ?? location;
  // What each line's variant has at the origin, so shipping can warn before a count goes
  // below zero. Only a draft ships.
  const atOrigin: Record<string, number> = {};
  if (transfer.status === "draft" && canWrite) {
    const stocks = await Promise.all(
      transfer.lines.map((l) =>
        client.GET("/admin/variants/{id}/inventory", { params: { path: { id: l.variant } } }),
      ),
    );
    transfer.lines.forEach((line, i) => {
      const level = stocks[i]?.data?.levels.find((lv) => lv.location === transfer.origin);
      atOrigin[line.variant] = level?.on_hand ?? 0;
    });
  }
  return (
    <VStack gap={5}>
      <PageHeader
        title={trNumber(transfer.number)}
        back={back}
        badges={<TransferStatusBadge status={transfer.status} t={t} />}
        description={`${nameOf(transfer.origin)} → ${nameOf(transfer.destination)}`}
        actions={
          canWrite && <TransferActions transfer={transfer} atOrigin={atOrigin} places={places} />
        }
      />
      <TransferView
        key={transfer.updated_at}
        transfer={transfer}
        places={places}
        isReadOnly={!canWrite}
      />
    </VStack>
  );
}
