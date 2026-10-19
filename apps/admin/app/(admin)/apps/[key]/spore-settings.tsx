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

/** One property of a spore's settings schema: the subset the API accepts (design note 0030, §8). */
type Property = {
  type: "string" | "integer" | "number" | "boolean";
  title?: string;
  description?: string;
  enum?: unknown[];
  "x-hypha-secret"?: boolean;
};

type Schema = { properties?: Record<string, Property>; required?: string[] };

type Value = string | number | boolean | null;

/** The form's start: each plain field as saved; secrets start empty (typing one replaces it). */
function initial(schema: Schema, settings: Record<string, unknown>): Record<string, Value> {
  const values: Record<string, Value> = {};
  for (const [name, property] of Object.entries(schema.properties ?? {})) {
    const saved = settings[name];
    if (property["x-hypha-secret"]) values[name] = "";
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
  const [fields, setFields] = useState<Record<string, string>>({});
  const [error, setError] = useState<string>();
  const [isSaved, setIsSaved] = useState(false);
  const [isPending, startTransition] = useTransition();
  const properties = Object.entries(schema.properties ?? {});
  const required = new Set(schema.required ?? []);

  const set = (name: string, value: Value) => {
    setValues({ ...values, [name]: value });
    setIsSaved(false);
  };

  const save = () =>
    startTransition(async () => {
      const body: Record<string, unknown> = {};
      for (const [name] of properties) {
        const value = values[name];
        // A secret left empty keeps what's saved; a plain field left empty is unset.
        if (value === "" || value === null) continue;
        body[name] = value;
      }
      const result = await saveSporeSettings(appKey, body);
      setError(result.error);
      setFields(result.fields ?? {});
      if (!result.error) {
        setIsSaved(true);
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
          const wasSet = isSet(settings[name]);
          return (
            <TextInput
              key={name}
              label={label}
              type="password"
              description={
                property.description ??
                (wasSet ? "Set. Type a new value to replace it." : "Not set yet.")
              }
              placeholder={wasSet ? "••••••••" : undefined}
              isRequired={required.has(name) && !wasSet}
              isDisabled={isDisabled}
              status={status}
              value={typeof value === "string" ? value : ""}
              onChange={(text) => set(name, text)}
            />
          );
        }
        if (property.enum) {
          return (
            <Selector
              key={name}
              label={label}
              options={property.enum.map((option) => String(option))}
              value={typeof value === "string" ? value : undefined}
              isDisabled={isDisabled}
              onChange={(option) => set(name, option)}
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
