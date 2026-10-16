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
import { ArrowDownIcon, ArrowUpIcon, TrashIcon } from "@heroicons/react/24/outline";
import { useState } from "react";

import {
  type Draft,
  type OptionDraft,
  newKey,
  orphaned,
  variantTitle,
  withAllCombinations,
} from "./draft";

/**
 * Options and their values (design note 0014, section 3), part of the editor's draft.
 * A variant using a removed value is named here and removed with the save; "Create all
 * variants" fills the draft with the combinations it lacks (design note 0021, Q1).
 */
export function OptionsEditor({
  draft,
  savedTitles,
  onChange,
}: {
  draft: Draft;
  /** Variant ID → its saved title ("M / Blue"), to name the ones a save would remove. */
  savedTitles: Record<string, string>;
  onChange: (change: Partial<Draft>) => void;
}) {
  // A message belongs to the options it was about: after a save or a discard the draft
  // gets new options, and the message goes with the old ones.
  const [message, setMessage] = useState<{
    status: "error" | "success";
    text: string;
    options: OptionDraft[];
  }>();
  const list = draft.options;
  const removed = orphaned(draft);

  const setList = (options: OptionDraft[]) => onChange({ options });
  const update = (key: string, change: (d: OptionDraft) => OptionDraft) =>
    setList(list.map((d) => (d.key === key ? change(d) : d)));
  const move = (from: number, to: number) => {
    const next = [...list];
    const [d] = next.splice(from, 1);
    next.splice(to, 0, d);
    setList(next);
  };
  const addValue = (d: OptionDraft) => {
    const label = d.adding.trim();
    if (label === "") return;
    update(d.key, (x) => ({ ...x, values: [...x.values, { key: newKey(), label }], adding: "" }));
  };
  const createAll = () => {
    const result = withAllCombinations(draft);
    if ("error" in result) {
      setMessage({ status: "error", text: result.error, options: draft.options });
      return;
    }
    const added = result.draft.variants.length - draft.variants.length;
    onChange({ options: result.draft.options, variants: result.draft.variants });
    setMessage({
      options: result.draft.options,
      status: "success",
      text:
        added === 0
          ? "Every combination already has a variant."
          : `Added ${added === 1 ? "a variant" : `${added} variants`} below: price them, then save.`,
    });
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

        {message && message.options === list && (
          <Banner status={message.status} title={message.text} />
        )}
        {removed.length > 0 && (
          <Banner
            status="warning"
            title={`Saving removes ${removed.length === 1 ? "a variant" : `${removed.length} variants`}: ${removed
              .map((v) => (v.id ? savedTitles[v.id] : undefined) ?? variantTitle(draft, v) ?? "new")
              .join(", ")}.`}
            description="They use values you removed. Their prices in price lists and their stock records stay behind until they're cleaned up."
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
                onClick={() => setList(list.filter((x) => x.key !== d.key))}
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

        <HStack gap={2}>
          <Button
            label="Add option"
            variant="secondary"
            onClick={() => setList([...list, { key: newKey(), name: "", values: [], adding: "" }])}
          />
          <Button
            label="Create all variants"
            variant="ghost"
            isDisabled={list.length === 0}
            tooltip="Adds a variant for every combination of values that doesn't have one."
            onClick={createAll}
          />
        </HStack>
      </VStack>
    </Card>
  );
}
