"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { Divider } from "@astryxdesign/core/Divider";
import { IconButton } from "@astryxdesign/core/IconButton";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { Token } from "@astryxdesign/core/Token";
import type { AdminSchemas } from "@hypha/sdk/admin";
import { ArrowDownIcon, ArrowUpIcon, TrashIcon } from "@heroicons/react/24/outline";
import { useState, useTransition } from "react";

import { type OptionDraft, setOptions } from "../actions";

type Option = AdminSchemas["OptionResponse"];

/** An option being edited; `key` is stable for React, `id` is the saved option's. */
type Draft = {
  key: string;
  id?: string;
  name: string;
  values: { key: string; id?: string; label: string }[];
  /** The "Add a value" input. */
  adding: string;
};

let nextKey = 0;
const newKey = () => `new-${nextKey++}`;

function drafts(options: Option[]): Draft[] {
  return options.map((o) => ({
    key: o.id,
    id: o.id,
    name: o.name,
    values: o.values.map((v) => ({ key: v.id, id: v.id, label: v.label })),
    adding: "",
  }));
}

function request(list: Draft[]): OptionDraft[] {
  return list.map((d) => ({
    id: d.id,
    name: d.name,
    values: d.values.map((v) => ({ id: v.id, label: v.label })),
  }));
}

/**
 * Options and their values (design note 0014, section 3), saved as one list. Removing a
 * value that variants use asks first, naming them, then removes them in the same save.
 */
export function OptionsEditor({
  id,
  options,
  variantTitles,
}: {
  id: string;
  options: Option[];
  /** Variant ID → its title ("M / Blue"), to name the ones a save would remove. */
  variantTitles: Record<string, string>;
}) {
  const [list, setList] = useState(() => drafts(options));
  const [error, setError] = useState<string>();
  const [toRemove, setToRemove] = useState<string[]>();
  const [isPending, startTransition] = useTransition();

  const saved = JSON.stringify(request(drafts(options)));
  const isDirty = JSON.stringify(request(list)) !== saved;

  const update = (key: string, change: (d: Draft) => Draft) =>
    setList((before) => before.map((d) => (d.key === key ? change(d) : d)));
  const move = (from: number, to: number) =>
    setList((before) => {
      const next = [...before];
      const [d] = next.splice(from, 1);
      next.splice(to, 0, d);
      return next;
    });
  const addValue = (d: Draft) => {
    const label = d.adding.trim();
    if (label === "") return;
    update(d.key, (x) => ({ ...x, values: [...x.values, { key: newKey(), label }], adding: "" }));
  };

  const save = (removeVariants: string[] = []) =>
    startTransition(async () => {
      const result = await setOptions(id, request(list), removeVariants);
      const variants = (result.details as { variants?: string[] } | null)?.variants ?? [];
      if (result.code === "option_value_in_use") {
        setError(undefined);
        setToRemove(variants);
        return;
      }
      setToRemove(undefined);
      if (result.code === "duplicate_option_combination" && variants.length === 2) {
        const [a, b] = variants.map((v) => variantTitles[v] ?? v);
        setError(`Variants ${a} and ${b} would become the same. Remove one of them first.`);
        return;
      }
      setError(result.error);
    });

  const reset = () => {
    setList(drafts(options));
    setError(undefined);
    setToRemove(undefined);
  };

  return (
    <Card>
      <VStack gap={4}>
        <VStack gap={1}>
          <Text type="display-3" as="h2">
            Options
          </Text>
          <Text type="supporting" color="secondary">
            What variants differ by, like Size or Color. A new option gives every variant its
            first value.
          </Text>
        </VStack>

        {error && <Banner status="error" title={error} />}
        {toRemove && (
          <Banner
            status="warning"
            title={`Saving removes ${toRemove.length === 1 ? "a variant" : `${toRemove.length} variants`}: ${toRemove
              .map((v) => variantTitles[v] ?? v)
              .join(", ")}.`}
            description="They use values you removed. Their prices in price lists and their stock records stay behind until they're cleaned up."
            endContent={
              <HStack gap={2}>
                <Button label="Cancel" variant="ghost" onClick={() => setToRemove(undefined)} />
                <Button
                  label="Remove and save"
                  variant="destructive"
                  isLoading={isPending}
                  onClick={() => save(toRemove)}
                />
              </HStack>
            }
          />
        )}

        {list.length === 0 && (
          <Text color="secondary">This product has a single variant and no options.</Text>
        )}

        {list.map((d, index) => (
          <VStack key={d.key} gap={3}>
            {index > 0 && <Divider />}
            <HStack gap={2} vAlign="end">
              <TextInput
                label="Option name"
                placeholder="Size"
                value={d.name}
                onChange={(name) => update(d.key, (x) => ({ ...x, name }))}
                width={240}
              />
              <IconButton
                label="Move up"
                tooltip="Move up"
                variant="ghost"
                icon={<ArrowUpIcon width={16} />}
                isDisabled={index === 0}
                onClick={() => move(index, index - 1)}
              />
              <IconButton
                label="Move down"
                tooltip="Move down"
                variant="ghost"
                icon={<ArrowDownIcon width={16} />}
                isDisabled={index === list.length - 1}
                onClick={() => move(index, index + 1)}
              />
              <IconButton
                label={`Remove ${d.name || "option"}`}
                tooltip="Remove option"
                variant="ghost"
                icon={<TrashIcon width={16} />}
                onClick={() => setList((before) => before.filter((x) => x.key !== d.key))}
              />
            </HStack>
            <HStack gap={2} wrap="wrap" vAlign="center">
              {d.values.map((v) => (
                <Token
                  key={v.key}
                  label={v.label}
                  onRemove={() =>
                    update(d.key, (x) => ({
                      ...x,
                      values: x.values.filter((y) => y.key !== v.key),
                    }))
                  }
                />
              ))}
              <TextInput
                label={`Add a value to ${d.name || "this option"}`}
                isLabelHidden
                placeholder="Add a value, then Enter"
                size="sm"
                value={d.adding}
                onChange={(adding) => update(d.key, (x) => ({ ...x, adding }))}
                onEnter={() => addValue(d)}
                width={200}
              />
            </HStack>
          </VStack>
        ))}

        <HStack gap={2} hAlign="between">
          <Button
            label="Add option"
            variant="secondary"
            onClick={() =>
              setList((before) => [...before, { key: newKey(), name: "", values: [], adding: "" }])
            }
          />
          <HStack gap={2}>
            {isDirty && <Button label="Discard" variant="ghost" onClick={reset} />}
            <Button
              label="Save options"
              variant="primary"
              isDisabled={!isDirty}
              isLoading={isPending && !toRemove}
              onClick={() => save()}
            />
          </HStack>
        </HStack>
      </VStack>
    </Card>
  );
}
