// Server-only: the custom field definitions pages read. Never import this from a "use
// client" file.

import type { Definition, OwnerType } from "./custom-fields";
import { loaded, perRequest, sessionClient } from "./hypha";

/**
 * Every field definition, by owner type then position, once per request. `null` without
 * `custom_fields:read`: pages then show no custom fields at all.
 */
export const loadFieldDefinitions = perRequest(async (): Promise<Definition[] | null> => {
  const client = await sessionClient();
  const result = await client.GET("/admin/custom-field-definitions");
  if (result.response.status === 403) return null;
  return loaded(result).items;
});

/** One owner type's definitions in their order; empty without the scope. */
export async function definitionsOf(type: OwnerType): Promise<Definition[]> {
  const all = await loadFieldDefinitions();
  return (all ?? [])
    .filter((d) => d.owner_type === type)
    .toSorted((a, b) => a.position - b.position);
}
