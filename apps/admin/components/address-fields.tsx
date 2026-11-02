"use client";

// An address form's fields as the country lays them out (`GET
// /admin/address-formats/{country}`): its order, rows, labels and required fields,
// the region as a select where the country lists regions. Problems come from the API's
// `invalid_address` (`details.problems`), each under its field.

import { Grid } from "@astryxdesign/core/Grid";
import { Selector } from "@astryxdesign/core/Selector";
import { VStack } from "@astryxdesign/core/Layout";
import { TextInput } from "@astryxdesign/core/TextInput";

import {
  type AddressField,
  type AddressFormat,
  type AddressValues,
  type FieldProblems,
  asField,
} from "@/lib/address";

/** The fields a form sends, `country` apart (it's the page's). */
type Field = Exclude<AddressField, "country">;

export function AddressFields({
  format,
  values,
  onChange,
  problems,
  tried,
  size,
}: {
  format: AddressFormat;
  values: AddressValues;
  onChange: (field: Field, value: string) => void;
  /** Each field's problem, from the last save. */
  problems?: FieldProblems;
  /** Whether a save was tried: empty required fields are marked from then on. */
  tried: boolean;
  size?: "lg";
}) {
  const required = new Set(format.required);
  const status = (field: Field) => {
    const problem = problems?.[field];
    if (problem) return { type: "error" as const, message: problem };
    if (tried && required.has(field) && values[field].trim() === "") {
      return { type: "error" as const, message: "Required" };
    }
    return undefined;
  };
  const input = (field: Field) => {
    const label = format.labels[field] ?? field;
    const isRequired = required.has(field);
    if (field === "province" && format.regions.length > 0) {
      return (
        <Selector
          key={field}
          size={size}
          label={label}
          isRequired={isRequired}
          isOptional={!isRequired}
          options={format.regions.map((r) => ({ value: r.code, label: r.name }))}
          value={values.province || undefined}
          onChange={(value) => onChange("province", value)}
          hasSearch={format.regions.length > 12}
          placeholder={`Choose a ${label.toLowerCase()}`}
          status={status(field)}
        />
      );
    }
    return (
      <TextInput
        key={field}
        size={size}
        label={label}
        isRequired={isRequired}
        isOptional={!isRequired}
        value={values[field]}
        onChange={(value) => onChange(field, value)}
        placeholder={
          field === "postal_code" ? (format.postal_code_example ?? undefined) : undefined
        }
        autoComplete={AUTOCOMPLETE[field]}
        status={status(field)}
      />
    );
  };
  return (
    <VStack gap={3}>
      {format.rows.map((row) => {
        const fields = row
          .map(asField)
          .filter((f): f is Field => f !== undefined && f !== "country");
        if (fields.length === 0) return null;
        if (fields.length === 1) return input(fields[0]);
        return (
          <Grid key={fields.join(" ")} columns={{ minWidth: 160 }} gap={3}>
            {fields.map(input)}
          </Grid>
        );
      })}
    </VStack>
  );
}

/** What browsers fill each field with. */
const AUTOCOMPLETE: Record<Field, string> = {
  first_name: "given-name",
  last_name: "family-name",
  company: "organization",
  line1: "address-line1",
  line2: "address-line2",
  city: "address-level2",
  province: "address-level1",
  postal_code: "postal-code",
  phone: "tel",
};
