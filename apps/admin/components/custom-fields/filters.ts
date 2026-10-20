// Filterable custom fields as list filters (design note 0033, §7): a choice field offers
// its choices (any of them), true/false fields a yes/no, the rest the value typed. The
// URL parameter is the API's own, `field[custom.material]`; the list's server page
// passes it on (`fieldQuery`).

import type { FilterDef } from "@/components/resource-table/resource-table";
import { type Definition, filterParam, isFilterOffered, parseType } from "@/lib/custom-fields";

export function customFieldFilters(definitions: Definition[]): FilterDef[] {
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
          { value: "true", label: "Yes" },
          { value: "false", label: "No" },
        ],
      };
    }
    return { kind: "text", param, label };
  });
}
