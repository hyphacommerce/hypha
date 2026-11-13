// Filterable custom fields as list filters: a choice field offers its choices (any of
// them), true/false fields a yes/no, the rest the value typed. The URL parameter is the
// API's own, `field[custom.material]`; the list's server page passes it on
// (`fieldQuery`).

import type { FilterDef } from "@/components/resource-table/resource-table";
import { type Definition, filterParam, isFilterOffered, parseType } from "@/lib/custom-fields";
import type { TFor } from "@/lib/messages";

/**
 * `t` names the yes/no of a true/false field; without one they're English, for the
 * lists that haven't a translator yet.
 */
export function customFieldFilters(definitions: Definition[], t?: TFor<"common">): FilterDef[] {
  return definitions.filter(isFilterOffered).map((definition): FilterDef => {
    const param = filterParam(definition);
    const label = definition.name;
    const { base } = parseType(definition.type);
    if (base === "choice") {
      return {
        kind: "multi",
        param,
        label,
        options: (definition.validations.choices ?? []).map((c) => ({
          value: c.key,
          label: c.label,
        })),
      };
    }
    if (base === "boolean") {
      return {
        kind: "select",
        param,
        label,
        options: [
          { value: "true", label: t?.("common.yes") ?? "Yes" },
          { value: "false", label: t?.("common.no") ?? "No" },
        ],
      };
    }
    return { kind: "text", param, label };
  });
}
