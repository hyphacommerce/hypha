"use client";

// The apps validating checkouts (`checkout.validate`): every installed app answering it is
// asked when a cart's address is set, when an option is chosen and at checkout. Per app,
// whether it's asked and whether checkout stops when it can't be.

import { Banner } from "@astryxdesign/core/Banner";
import { Card } from "@astryxdesign/core/Card";
import { VStack } from "@astryxdesign/core/Layout";
import { Switch } from "@astryxdesign/core/Switch";
import {
  Table,
  TableBody,
  TableCell,
  TableHeader,
  TableHeaderCell,
  TableRow,
} from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";
import { useState } from "react";

import { type ValidationApp, setValidationApps } from "./actions";

export function ValidationAppsCard({
  apps,
  names,
  canWrite,
}: {
  /** Every app answering `checkout.validate`, with its setting. */
  apps: ValidationApp[];
  /** Apps' names by key, where the caller may read them. */
  names: Record<string, string>;
  canWrite: boolean;
}) {
  const [error, setError] = useState<string>();
  // The whole list goes with each change: the API replaces what was set.
  const change = async (app: string, part: Partial<ValidationApp>) => {
    const next = apps.map((a) => (a.app === app ? { ...a, ...part } : a));
    setError((await setValidationApps(next)).error);
  };

  return (
    <Card>
      <VStack gap={3}>
        <VStack gap={1}>
          <Text type="display-3" as="h2">
            Validation apps
          </Text>
          <Text color="secondary">
            Apps that check a cart as the shopper goes: when the address is set, when an option is
            chosen and at checkout. Their errors stop checkout and their warnings show under the
            field. At most five.
          </Text>
        </VStack>
        {error && <Banner status="error" title={error} />}
        {apps.length === 0 ? (
          <Text color="secondary">
            No installed app validates checkouts. Install one whose manifest declares
            checkout.validate and is granted carts:read.
          </Text>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHeaderCell>App</TableHeaderCell>
                <TableHeaderCell>Asked</TableHeaderCell>
                <TableHeaderCell>Block checkout when this app fails</TableHeaderCell>
              </TableRow>
            </TableHeader>
            <TableBody>
              {apps.map((a) => {
                const name = names[a.app] ?? a.app;
                return (
                  <TableRow key={a.app}>
                    <TableCell>
                      <VStack gap={0.5}>
                        <Text>{name}</Text>
                        {name !== a.app && (
                          <Text type="supporting" color="secondary">
                            {a.app}
                          </Text>
                        )}
                      </VStack>
                    </TableCell>
                    <TableCell>
                      <Switch
                        label={`Ask ${name}`}
                        isLabelHidden
                        size="sm"
                        value={a.enabled}
                        isDisabled={!canWrite}
                        changeAction={(enabled) => change(a.app, { enabled })}
                      />
                    </TableCell>
                    <TableCell>
                      <Switch
                        label={`Block checkout when ${name} fails`}
                        isLabelHidden
                        size="sm"
                        value={a.block_on_failure}
                        isDisabled={!canWrite || !a.enabled}
                        changeAction={(block_on_failure) => change(a.app, { block_on_failure })}
                      />
                    </TableCell>
                  </TableRow>
                );
              })}
            </TableBody>
          </Table>
        )}
      </VStack>
    </Card>
  );
}
