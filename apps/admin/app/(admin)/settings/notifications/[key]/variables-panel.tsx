"use client";

// What a template can print (design note 0028, §11): every path of its sample context,
// with the sample's value. Clicking one copies the Jinja that prints it.

import { Card } from "@astryxdesign/core/Card";
import { VStack } from "@astryxdesign/core/Layout";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { useToast } from "@astryxdesign/core/Toast";
import { MagnifyingGlassIcon } from "@heroicons/react/24/outline";
import { useState } from "react";

import { type Variable, printable } from "@/lib/notifications";

/** The snippet a click copies: a loop for a list, else the value. */
function snippet(variable: Variable): string {
  if (!variable.isList) return printable(variable.path);
  const item = variable.path.split(".").at(-1)?.replace(/s$/, "") || "item";
  return `{% for ${item} in ${variable.path} %}{% endfor %}`;
}

export function VariablesPanel({ variables }: { variables: Variable[] }) {
  const [filter, setFilter] = useState("");
  const toast = useToast();
  const shown = variables.filter((v) => v.path.toLowerCase().includes(filter.trim().toLowerCase()));

  const copy = async (variable: Variable) => {
    const text = snippet(variable);
    try {
      await navigator.clipboard.writeText(text);
      toast({ body: `Copied ${text}`, uniqueID: "variable-copied" });
    } catch {
      toast({ body: "Couldn't copy: select the path and copy it yourself.", type: "error" });
    }
  };

  return (
    <Card>
      <VStack gap={3}>
        <VStack gap={1}>
          <Text type="display-3" as="h2">
            Variables
          </Text>
          <Text type="supporting" color="secondary">
            Click one to copy it. Values are from a made-up order. A list&apos;s items are shown
            under [0]: loop over the list to print each.
          </Text>
        </VStack>
        <TextInput
          label="Filter variables"
          isLabelHidden
          placeholder="Filter, like order.total"
          startIcon={MagnifyingGlassIcon}
          hasClear
          width="100%"
          value={filter}
          onChange={setFilter}
        />
        <div className="variables-list">
          {shown.length === 0 && <Text color="secondary">No variable matches.</Text>}
          {shown.map((variable) => (
            <button
              key={variable.path}
              type="button"
              className="variable"
              title={`Copy ${snippet(variable)}`}
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
