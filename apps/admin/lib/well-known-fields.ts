// The well-known customer fields: `custom.tags` (labels) and `custom.note` (the staff
// note). They're ordinary merchant definitions the API suggests (`GET
// /admin/custom-field-definitions/suggestions`); the admin offers them in one click and
// renders them in place (chips under the name, a note card) when they exist with the
// shape it expects. Safe for client components: types and pure functions only.

import type { AdminSchemas } from "@hypha/sdk/admin";

import type { Definition } from "./custom-fields";

export type Suggestion = AdminSchemas["SuggestedDefinitionResponse"];
export type CreateDefinitionRequest = AdminSchemas["CreateCustomFieldDefinitionRequest"];

/** The body that creates a suggested field: the suggestion minus what only describes it. */
export function suggestionBody(s: Suggestion): CreateDefinitionRequest {
  return {
    owner_type: s.owner_type,
    key: s.key,
    type: s.type,
    name: s.name,
    description: s.description,
    validations: s.validations,
    access: s.access,
    personal: s.personal,
    capabilities: s.capabilities,
  };
}

export const WELL_KNOWN = {
  labels: { key: "tags", field: "custom.tags", type: "list.text" },
  note: { key: "note", field: "custom.note", type: "multiline_text" },
} as const;

export type WellKnown = keyof typeof WELL_KNOWN;

const KINDS: WellKnown[] = ["labels", "note"];

/**
 * The merchant's definition of a well-known customer field, when it has the shape the
 * admin renders in place. A `custom.tags` of another type is an ordinary field on the
 * custom fields card.
 */
export function wellKnownField(
  definitions: Definition[],
  which: WellKnown,
): Definition | undefined {
  const expected = WELL_KNOWN[which];
  return definitions.find(
    (d) => d.owner_type === "customer" && d.field === expected.field && d.type === expected.type,
  );
}

/** The customer's definitions without those rendered in place. */
export function withoutWellKnown(definitions: Definition[]): Definition[] {
  const inPlace = new Set(
    KINDS.map((which) => wellKnownField(definitions, which)?.id).filter((id) => id !== undefined),
  );
  return definitions.filter((d) => !inPlace.has(d.id));
}

/** A list field's items as text, as stored (`[]` for none or another shape). */
export function textItems(value: unknown): string[] {
  return Array.isArray(value) ? value.filter((v): v is string => typeof v === "string") : [];
}
