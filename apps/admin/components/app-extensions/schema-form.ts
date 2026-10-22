// A JSON Schema for an action's input, read as a flat form: an object whose properties
// are strings, numbers, integers, booleans or enums. That's what admin actions take;
// anything else isn't shown as a field (and the app checks the input it gets anyway).

export type SchemaField =
  | { name: string; label: string; description?: string; required: boolean; kind: "text" }
  | {
      name: string;
      label: string;
      description?: string;
      required: boolean;
      kind: "number";
      integer: boolean;
      min?: number;
      max?: number;
    }
  | { name: string; label: string; description?: string; required: boolean; kind: "boolean" }
  | {
      name: string;
      label: string;
      description?: string;
      required: boolean;
      kind: "enum";
      options: string[];
    };

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function text(value: unknown): string | undefined {
  return typeof value === "string" && value.trim() !== "" ? value : undefined;
}

/** The form's fields, in the schema's order; none for a schema that isn't a flat object. */
export function schemaFields(schema: unknown): SchemaField[] {
  if (!isRecord(schema) || !isRecord(schema.properties)) return [];
  const required = new Set(
    Array.isArray(schema.required) ? schema.required.filter((r) => typeof r === "string") : [],
  );
  const fields: SchemaField[] = [];
  for (const [name, property] of Object.entries(schema.properties)) {
    if (!isRecord(property)) continue;
    const base = {
      name,
      label: text(property.title) ?? name,
      description: text(property.description),
      required: required.has(name),
    };
    if (Array.isArray(property.enum)) {
      const options = property.enum.filter((o): o is string => typeof o === "string");
      if (options.length > 0) fields.push({ ...base, kind: "enum", options });
      continue;
    }
    switch (property.type) {
      case "string":
        fields.push({ ...base, kind: "text" });
        break;
      case "number":
      case "integer":
        fields.push({
          ...base,
          kind: "number",
          integer: property.type === "integer",
          min: typeof property.minimum === "number" ? property.minimum : undefined,
          max: typeof property.maximum === "number" ? property.maximum : undefined,
        });
        break;
      case "boolean":
        fields.push({ ...base, kind: "boolean" });
        break;
      default:
        break;
    }
  }
  return fields;
}

export type FormValue = string | number | boolean | null;

/** What's missing or out of range, by field name. */
export function checkValues(
  fields: SchemaField[],
  values: Record<string, FormValue>,
): Record<string, string> {
  const problems: Record<string, string> = {};
  for (const field of fields) {
    const value = values[field.name];
    const empty = value === null || value === undefined || value === "";
    if (empty) {
      if (field.required && field.kind !== "boolean") problems[field.name] = "Required";
      continue;
    }
    if (field.kind === "number" && typeof value === "number") {
      if (field.integer && !Number.isInteger(value)) problems[field.name] = "A whole number";
      else if (field.min !== undefined && value < field.min)
        problems[field.name] = `At least ${field.min}`;
      else if (field.max !== undefined && value > field.max)
        problems[field.name] = `At most ${field.max}`;
    }
  }
  return problems;
}

/** The input to send: the fields that have values (booleans always). */
export function inputOf(
  fields: SchemaField[],
  values: Record<string, FormValue>,
): Record<string, unknown> {
  const input: Record<string, unknown> = {};
  for (const field of fields) {
    const value = values[field.name];
    if (field.kind === "boolean") input[field.name] = value === true;
    else if (value !== null && value !== undefined && value !== "") input[field.name] = value;
  }
  return input;
}
