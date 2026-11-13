"use client";

// What a template can print: every path of its sample context, with the sample's
// value. Clicking one copies the Jinja that prints it.

import { Card } from "@astryxdesign/core/Card";
import { VStack } from "@astryxdesign/core/Layout";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { useToast } from "@astryxdesign/core/Toast";
import { MagnifyingGlassIcon } from "@heroicons/react/24/outline";
import { useState } from "react";

import { type Variable, printable } from "@/lib/notifications";
import { useT } from "@/lib/use-t";

/** The snippet a click copies: a loop for a list, else the value. */
function snippet(variable: Variable): string {
  if (!variable.isList) return printable(variable.path);
  const item = variable.path.split(".").at(-1)?.replace(/s$/, "") || "item";
  return `{% for ${item} in ${variable.path} %}{% endfor %}`;
}

export function VariablesPanel({ variables }: { variables: Variable[] }) {
  const t = useT<"notifications">();
  const [filter, setFilter] = useState("");
  const toast = useToast();
  const shown = variables.filter((v) => v.path.toLowerCase().includes(filter.trim().toLowerCase()));

  const copy = async (variable: Variable) => {
    const text = snippet(variable);
    try {
      await navigator.clipboard.writeText(text);
      toast({
        body: t("notifications.variables_copied", { snippet: text }),
        uniqueID: "variable-copied",
      });
    } catch {
      toast({ body: t("notifications.variables_copy_failed"), type: "error" });
    }
  };

  return (
    <Card>
      <VStack gap={3}>
        <VStack gap={1}>
          <Text type="display-3" as="h2">
            {t("notifications.variables_title")}
          </Text>
          <Text type="supporting" color="secondary">
            {t("notifications.variables_description")}
          </Text>
        </VStack>
        <TextInput
          label={t("notifications.variables_filter_label")}
          isLabelHidden
          placeholder={t("notifications.variables_filter_placeholder")}
          startIcon={MagnifyingGlassIcon}
          hasClear
          width="100%"
          value={filter}
          onChange={setFilter}
        />
        <div className="variables-list">
          {shown.length === 0 && <Text color="secondary">{t("notifications.variables_none")}</Text>}
          {shown.map((variable) => (
            <button
              key={variable.path}
              type="button"
              className="variable"
              title={t("notifications.variables_copy", { snippet: snippet(variable) })}
              onClick={() => copy(variable)}
            >
              <code>{variable.path}</code>
              <span className="variable-sample">{variable.sample}</span>
            </button>
          ))}
        </div>
      </VStack>
    </Card>
  );
}
