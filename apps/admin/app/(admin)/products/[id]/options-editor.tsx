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
import { ArrowDownIcon, ArrowUpIcon, PlusIcon, TrashIcon } from "@heroicons/react/24/outline";
import { useState } from "react";

import { type Draft, type OptionDraft, newKey, orphaned, variantTitle } from "./draft";

/** Names offered for a new option, the ones most products use. */
const COMMON_NAMES = ["Size", "Color", "Material", "Style"];

/** Labels typed or pasted into "Add values", split on commas and new lines. */
const splitLabels = (text: string) =>
  text
    .split(/[,\n]/)
    .map((label) => label.trim())
    .filter((label) => label !== "");

/**
 * Options and their values (design note 0014, section 3), part of the editor's draft.
 * An option shows its values; Edit opens it to rename, reorder, add and remove them
 * (a renamed value keeps its variants). A variant using a removed value is named here
 * and removed with the save.
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
  // Which options are open for editing; a new one opens.
  const [open, setOpen] = useState<Set<string>>(new Set());
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
  const toggle = (key: string, isOpen: boolean) =>
    setOpen((before) => {
      const next = new Set(before);
      if (isOpen) next.add(key);
      else next.delete(key);
      return next;
    });
  const add = () => {
    const key = newKey();
    setList([...list, { key, name: "", values: [], adding: "" }]);
    toggle(key, true);
  };

  return (
    <Card>
      <VStack gap={4}>
        <HStack gap={2} hAlign="between" vAlign="center">
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              Options
            </Text>
            <Text type="supporting" color="secondary">
              What variants differ by, like Size or Color.
            </Text>
          </VStack>
          {list.length > 0 && (
            <Button
              label="Add option"
              variant="secondary"
              size="sm"
              icon={<PlusIcon width={16} />}
              onClick={add}
            />
          )}
        </HStack>

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
          <VStack gap={3} hAlign="start">
            <Text color="secondary">
              This product is sold one way. Add options if it comes in several sizes, colors or
              materials: each combination becomes a variant with its own price, SKU and stock.
            </Text>
            <Button
              label="Add options"
              variant="secondary"
              icon={<PlusIcon width={16} />}
              onClick={add}
            />
          </VStack>
        )}

        {list.map((d, index) => (
          <VStack key={d.key} gap={3}>
            {index > 0 && <Divider />}
            {open.has(d.key) ? (
              <OptionForm
                option={d}
                isFirst={index === 0}
                isLast={index === list.length - 1}
                onChange={(change) => update(d.key, change)}
                onMove={(by) => move(index, index + by)}
                onRemove={() => {
                  setList(list.filter((x) => x.key !== d.key));
                  toggle(d.key, false);
                }}
                onDone={() => {
                  update(d.key, commitTyped);
                  toggle(d.key, false);
                }}
              />
            ) : (
              <HStack gap={3} hAlign="between" vAlign="center">
                <VStack gap={2}>
                  <Text weight="medium">{d.name || "Unnamed option"}</Text>
                  <HStack gap={1} wrap="wrap">
                    {d.values.length === 0 ? (
                      <Text type="supporting" color="secondary">
                        No values yet
                      </Text>
                    ) : (
                      d.values.map((v) => <Token key={v.key} label={v.label} size="sm" />)
                    )}
                  </HStack>
                </VStack>
                <Button
                  label="Edit"
                  variant="ghost"
                  size="sm"
                  onClick={() => toggle(d.key, true)}
                />
              </HStack>
            )}
          </VStack>
        ))}
      </VStack>
    </Card>
  );
}

/** The option with whatever was typed in "Add values" added as values. */
function commitTyped(option: OptionDraft): OptionDraft {
  const labels = splitLabels(option.adding);
  return {
    ...option,
    adding: "",
    values: [
      ...option.values,
      ...withoutTaken(option, labels).map((label) => ({ key: newKey(), label })),
    ],
  };
}

/** `labels` without the ones the option already has (ignoring case) or repeats. */
function withoutTaken(option: OptionDraft, labels: string[]): string[] {
  const taken = new Set(option.values.map((v) => v.label.trim().toLowerCase()));
  return labels.filter((label) => {
    const lower = label.toLowerCase();
    if (taken.has(lower)) return false;
    taken.add(lower);
    return true;
  });
}

/** One option, open: its name, its values as a list to rename, reorder and remove. */
function OptionForm({
  option,
  isFirst,
  isLast,
  onChange,
  onMove,
  onRemove,
  onDone,
}: {
  option: OptionDraft;
  isFirst: boolean;
  isLast: boolean;
  onChange: (change: (d: OptionDraft) => OptionDraft) => void;
  onMove: (by: -1 | 1) => void;
  onRemove: () => void;
  onDone: () => void;
}) {
  const setValues = (values: OptionDraft["values"]) => onChange((d) => ({ ...d, values }));
  const moveValue = (from: number, to: number) => {
    const next = [...option.values];
    const [v] = next.splice(from, 1);
    next.splice(to, 0, v);
    setValues(next);
  };
  const labels = option.values.map((v) => v.label.trim().toLowerCase());
  const isRepeated = (i: number) => labels[i] !== "" && labels.indexOf(labels[i]) !== i;

  return (
    <VStack gap={3}>
      <HStack gap={2} vAlign="end">
        <TextInput
          label="Option name"
          placeholder="Size"
          value={option.name}
          onChange={(name) => onChange((d) => ({ ...d, name }))}
          width={240}
          hasAutoFocus={option.name === ""}
        />
        <IconButton
          label="Move option up"
          tooltip="Move up"
          variant="ghost"
          icon={<ArrowUpIcon width={16} />}
          isDisabled={isFirst}
          onClick={() => onMove(-1)}
        />
        <IconButton
          label="Move option down"
          tooltip="Move down"
          variant="ghost"
          icon={<ArrowDownIcon width={16} />}
          isDisabled={isLast}
          onClick={() => onMove(1)}
        />
      </HStack>
      {option.name === "" && (
        <HStack gap={1} wrap="wrap" vAlign="center">
          <Text type="supporting" color="secondary">
            Common:
          </Text>
          {COMMON_NAMES.map((name) => (
            <Button
              key={name}
              label={name}
              size="sm"
              variant="ghost"
              onClick={() => onChange((d) => ({ ...d, name }))}
            />
          ))}
        </HStack>
      )}

      <VStack gap={2}>
        <VStack gap={0}>
          <Text type="supporting" weight="medium">
            Values
          </Text>
          <Text type="supporting" color="secondary">
            Renaming a value keeps its variants. Type several separated by commas, or press Enter
            after each.
          </Text>
        </VStack>
        {option.values.map((v, i) => (
          <HStack key={v.key} gap={1} vAlign="center">
            <TextInput
              label={`Value ${i + 1} of ${option.name || "this option"}`}
              isLabelHidden
              size="sm"
              value={v.label}
              onChange={(label) =>
                setValues(option.values.map((x) => (x.key === v.key ? { ...x, label } : x)))
              }
              status={
                v.label.trim() === ""
                  ? { type: "error", message: "Enter a value or remove it." }
                  : isRepeated(i)
                    ? { type: "error", message: "This option already has this value." }
                    : undefined
              }
              statusVariant="detached"
              width={240}
            />
            <IconButton
              label={`Move ${v.label || "value"} up`}
              tooltip="Move up"
              size="sm"
              variant="ghost"
              icon={<ArrowUpIcon width={14} />}
              isDisabled={i === 0}
              onClick={() => moveValue(i, i - 1)}
            />
            <IconButton
              label={`Move ${v.label || "value"} down`}
              tooltip="Move down"
              size="sm"
              variant="ghost"
              icon={<ArrowDownIcon width={14} />}
              isDisabled={i === option.values.length - 1}
              onClick={() => moveValue(i, i + 1)}
            />
            <IconButton
              label={`Remove ${v.label || "value"}`}
              tooltip="Remove value"
              size="sm"
              variant="ghost"
              icon={<TrashIcon width={14} />}
              onClick={() => setValues(option.values.filter((x) => x.key !== v.key))}
            />
          </HStack>
        ))}
        <TextInput
          label={`Add values to ${option.name || "this option"}`}
          isLabelHidden
          size="sm"
          placeholder={option.values.length === 0 ? "S, M, L" : "Add a value"}
          value={option.adding}
          onChange={(adding) =>
            // A pasted or typed comma adds what's before it.
            /[,\n]/.test(adding)
              ? onChange((d) => {
                  const parts = adding.split(/[,\n]/);
                  const rest = parts.pop() ?? "";
                  return {
                    ...commitTyped({ ...d, adding: parts.join(",") }),
                    adding: rest.trimStart(),
                  };
                })
              : onChange((d) => ({ ...d, adding }))
          }
          onEnter={() => onChange(commitTyped)}
          width={240}
        />
      </VStack>

      <HStack gap={2}>
        <Button label="Done" variant="secondary" size="sm" onClick={onDone} />
        <Button
          label="Remove option"
          variant="ghost"
          size="sm"
          icon={<TrashIcon width={16} />}
          onClick={onRemove}
        />
      </HStack>
    </VStack>
  );
}
