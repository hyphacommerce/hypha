"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { NumberInput } from "@astryxdesign/core/NumberInput";
import { Selector } from "@astryxdesign/core/Selector";
import { Switch } from "@astryxdesign/core/Switch";
import { TextInput } from "@astryxdesign/core/TextInput";
import { useState, useTransition } from "react";

import { saveSporeSettings } from "../actions";

/** One property of a spore's settings schema: the subset the API accepts. */
type Property = {
  type: "string" | "integer" | "number" | "boolean";
  title?: string;
  description?: string;
  enum?: unknown[];
  "x-hypha-secret"?: boolean;
};

type Schema = {
  properties?: Record<string, Property>;
  required?: string[];
  /** The fields' order (JSON objects carry none through the API): the rest by name. */
  "x-hypha-order"?: string[];
};

/** The schema's fields, in its `x-hypha-order`, then the rest by name. */
function ordered(schema: Schema): [string, Property][] {
  const order = schema["x-hypha-order"] ?? [];
  const rank = (name: string) => {
    const i = order.indexOf(name);
    return i === -1 ? order.length : i;
  };
  return Object.entries(schema.properties ?? {}).toSorted(
    ([a], [b]) => rank(a) - rank(b) || a.localeCompare(b),
  );
}

type Value = string | number | boolean | null;

/** Whether a schema's option (or a saved setting) is a value the form holds. */
function isValue(value: unknown): value is Value {
  return (
    value === null ||
    typeof value === "string" ||
    typeof value === "number" ||
    typeof value === "boolean"
  );
}

/** The form's start: each plain field as saved; secrets start empty (typing one replaces it). */
function initial(schema: Schema, settings: Record<string, unknown>): Record<string, Value> {
  const values: Record<string, Value> = {};
  for (const [name, property] of Object.entries(schema.properties ?? {})) {
    const saved = settings[name];
    if (property["x-hypha-secret"]) values[name] = "";
    // A choice keeps its option's own type (a number stays a number).
    else if (property.enum)
      values[name] =
        isValue(saved) && property.enum.some((option) => option === saved) ? saved : null;
    else if (property.type === "boolean") values[name] = saved === true;
    else if (property.type === "integer" || property.type === "number")
      values[name] = typeof saved === "number" ? saved : null;
    else values[name] = typeof saved === "string" ? saved : "";
  }
  return values;
}

/** Whether a secret field is set: the API answers it as `{"set": true}`, never its value. */
function isSet(value: unknown): boolean {
  return typeof value === "object" && value !== null && (value as { set?: boolean }).set === true;
}

/**
 * A built-in spore's settings form, drawn from its JSON Schema: text, choices, numbers and
 * switches. Secret fields are write-only: they show whether they're set, and are sent only
 * when typed.
 */
export function SporeSettingsForm({
  appKey,
  schema,
  settings,
  canWrite,
}: {
  appKey: string;
  schema: Schema;
  settings: Record<string, unknown>;
  canWrite: boolean;
}) {
  const [values, setValues] = useState(() => initial(schema, settings));
  // Secrets the merchant asked to clear: sent as `null` on the next save.
  const [cleared, setCleared] = useState<ReadonlySet<string>>(new Set());
  const [fields, setFields] = useState<Record<string, string>>({});
  const [error, setError] = useState<string>();
  const [isSaved, setIsSaved] = useState(false);
  const [isPending, startTransition] = useTransition();
  const properties = ordered(schema);
  const required = new Set(schema.required ?? []);

  const set = (name: string, value: Value) => {
    setValues({ ...values, [name]: value });
    setIsSaved(false);
  };

  const clear = (name: string) => {
    setCleared(new Set([...cleared, name]));
    set(name, "");
  };

  const save = () =>
    startTransition(async () => {
      const body: Record<string, unknown> = {};
      for (const [name] of properties) {
        const value = values[name];
        // A secret asked to be cleared is `null`; one left empty keeps what's saved; a
        // plain field left empty is unset.
        if (cleared.has(name) && (value === "" || value === null)) body[name] = null;
        else if (value === "" || value === null) continue;
        else body[name] = value;
      }
      const result = await saveSporeSettings(appKey, body);
      setError(result.error);
      setFields(result.fields ?? {});
      if (!result.error) {
        setIsSaved(true);
        setCleared(new Set());
        setValues(initial(schema, { ...body }));
      }
    });

  return (
    <VStack gap={3}>
      {properties.map(([name, property]) => {
        const label = property.title ?? name;
        const status = fields[name] ? { type: "error" as const, message: fields[name] } : undefined;
        const isDisabled = !canWrite;
        const value = values[name];
        if (property["x-hypha-secret"]) {
          const isClearing = cleared.has(name);
          const wasSet = isSet(settings[name]) && !isClearing;
          const description = isClearing
            ? "Cleared when you save."
            : wasSet
              ? "Set. Type a new value to replace it."
              : "Not set yet.";
          return (
            <VStack key={name} gap={1}>
              <TextInput
                label={label}
                type="password"
                // Whether it's set comes first: a spore's own description (where to find
                // the key) mustn't hide it.
                description={
                  property.description ? `${description} ${property.description}` : description
                }
                placeholder={wasSet ? "••••••••" : undefined}
                isRequired={required.has(name) && !wasSet}
                isDisabled={isDisabled}
                status={status}
                value={typeof value === "string" ? value : ""}
                onChange={(text) => {
                  if (isClearing) setCleared(new Set([...cleared].filter((n) => n !== name)));
                  set(name, text);
                }}
              />
              {wasSet && !required.has(name) && canWrite && (
                <HStack hAlign="end">
                  <Button label={`Clear ${label}`} size="sm" onClick={() => clear(name)} />
                </HStack>
              )}
            </VStack>
          );
        }
        if (property.enum) {
          const options = property.enum;
          return (
            <Selector
              key={name}
              label={label}
              options={options.map((option) => String(option))}
              value={value === null ? undefined : String(value)}
              isDisabled={isDisabled}
              // Back to the option itself: `2`, not `"2"`.
              onChange={(text) => {
                const option = options.find((o) => String(o) === text);
                set(name, isValue(option) ? option : null);
              }}
            />
          );
        }
        if (property.type === "boolean") {
          return (
            <Switch
              key={name}
              label={label}
              description={property.description}
              value={value === true}
              isDisabled={isDisabled}
              onChange={(checked) => set(name, checked)}
            />
          );
        }
        if (property.type === "integer" || property.type === "number") {
          return (
            <NumberInput
              key={name}
              label={label}
              description={property.description}
              isRequired={required.has(name)}
              isDisabled={isDisabled}
              value={typeof value === "number" ? value : null}
              onChange={(number) => set(name, number)}
            />
          );
        }
        return (
          <TextInput
            key={name}
            label={label}
            description={property.description}
            isRequired={required.has(name)}
            isDisabled={isDisabled}
            status={status}
            value={typeof value === "string" ? value : ""}
            onChange={(text) => set(name, text)}
          />
        );
      })}
      {error && <Banner status="error" title={error} onDismiss={() => setError(undefined)} />}
      {isSaved && (
        <Banner status="success" title="Settings saved." onDismiss={() => setIsSaved(false)} />
      )}
      {canWrite && (
        <HStack hAlign="end">
          <Button label="Save settings" variant="primary" isLoading={isPending} onClick={save} />
        </HStack>
      )}
    </VStack>
  );
}
