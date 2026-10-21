"use client";

// The customer's groups (design note 0032, §6), set whole from the groups the store has.

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { MultiSelector } from "@astryxdesign/core/MultiSelector";
import { Text } from "@astryxdesign/core/Text";
import { Token } from "@astryxdesign/core/Token";
import { useRouter } from "next/navigation";
import { useState, useTransition } from "react";

import type { Group } from "../groups/groups";
import { setCustomerGroups } from "../groups/actions";

export function GroupsCard({
  customerId,
  groups,
  current,
  canEdit,
}: {
  customerId: string;
  /** Every group the store has. */
  groups: Group[];
  /** The IDs of the groups the customer is in. */
  current: string[];
  /** `customers:write`. */
  canEdit: boolean;
}) {
  const router = useRouter();
  const [selected, setSelected] = useState(current);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const isDirty =
    selected.length !== current.length || selected.some((id) => !current.includes(id));
  const byId = new Map(groups.map((g) => [g.id, g]));

  return (
    <Card>
      <VStack gap={3}>
        <VStack gap={1}>
          <Text type="display-3" as="h2">
            Groups
          </Text>
          <Text type="supporting" color="secondary">
            Sets that prices and payment methods can refer to.{" "}
            <Link href="/customers/groups">Manage groups</Link>
          </Text>
        </VStack>
        {error && <Banner status="error" title={error} />}
        {groups.length === 0 ? (
          <Text color="secondary">No groups yet.</Text>
        ) : canEdit ? (
          <>
            <MultiSelector
              label="Groups"
              isLabelHidden
              options={groups.map((g) => ({ value: g.id, label: g.name }))}
              value={selected}
              onChange={(next) => {
                setSelected(next);
                setError(undefined);
              }}
              hasSearch={groups.length > 8}
              triggerDisplay="labels"
              width="100%"
            />
            {isDirty && (
              <HStack gap={2} hAlign="end">
                <Button
                  label="Discard"
                  variant="ghost"
                  size="sm"
                  onClick={() => setSelected(current)}
                />
                <Button
                  label="Save"
                  variant="primary"
                  size="sm"
                  isLoading={isPending}
                  onClick={() =>
                    startTransition(async () => {
                      const result = await setCustomerGroups(customerId, selected);
                      if (result.error) setError(result.error);
                      else router.refresh();
                    })
                  }
                />
              </HStack>
            )}
          </>
        ) : current.length === 0 ? (
          <Text color="secondary">In no group.</Text>
        ) : (
          <HStack gap={1} wrap="wrap">
            {current.map((id) => (
              <Token
                key={id}
                label={byId.get(id)?.name ?? id}
                size="sm"
                href={`/customers?group=${id}`}
              />
            ))}
          </HStack>
        )}
      </VStack>
    </Card>
  );
}
