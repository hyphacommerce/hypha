// The reference renderer for blocks, on Astryx, the components our admin uses: a block
// looks like the rest of the admin and runs no app code. A host with another design
// system writes its own from schema/block.v1.json.

/* oxlint-disable react/no-array-index-key -- a block is a fixed tree the app answered, never reordered or edited in place: a child's position is its identity, and elements carry no IDs */

import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { CheckboxInput } from "@astryxdesign/core/CheckboxInput";
import { Divider } from "@astryxdesign/core/Divider";
import { Heading } from "@astryxdesign/core/Heading";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { List, ListItem } from "@astryxdesign/core/List";
import { MetadataList, MetadataListItem } from "@astryxdesign/core/MetadataList";
import { NumberInput } from "@astryxdesign/core/NumberInput";
import { Selector } from "@astryxdesign/core/Selector";
import { Table, type TableColumn } from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { type FormEvent, type ReactNode, useState, useTransition } from "react";

import {
  type BlockElement,
  isElementType,
  type FieldElement,
  type FormElement,
  type FormValues,
  type KnownElement,
  type TableElement,
} from "./types.ts";

export interface HyphaBlockProps {
  /** The block's tree, as the app answered it (validate it first with `validateBlock`). */
  tree: { version: number; root: BlockElement };
  /** A button's action, or a form's with its values. A promise shows the control as busy. */
  onAction?: (action: string, values?: FormValues) => void | Promise<void>;
  /** A button's `navigate` path, within the app's page. */
  onNavigate?: (path: string) => void;
}

interface Handlers {
  onAction: HyphaBlockProps["onAction"];
  onNavigate: HyphaBlockProps["onNavigate"];
}

/** Renders a version-1 tree; anything else, and unknown elements, render nothing. */
export function HyphaBlock({ tree, onAction, onNavigate }: HyphaBlockProps) {
  if (tree.version !== 1) {
    warn(`block version ${String(tree.version)} isn't supported: nothing rendered`);
    return null;
  }
  return <Element element={tree.root} handlers={{ onAction, onNavigate }} />;
}

function warn(message: string) {
  // Development only: an older host meeting a newer element is expected in production.
  if (typeof process !== "undefined" && process.env.NODE_ENV === "production") return;
  console.warn(`[@hyphacommerce/app-blocks] ${message}`);
}

const STACK_GAP = { 0: 0, 1: 1, 2: 2, 3: 3, 4: 4 } as const;

function isKnown(element: BlockElement): element is KnownElement {
  return isElementType(element.type);
}

function Children({ elements, handlers }: { elements: BlockElement[]; handlers: Handlers }) {
  // A block is a fixed tree from the app, never reordered, so a child's place is its identity.
  return elements.map((child, place) => (
    <Element key={`${place}:${child.type}`} element={child} handlers={handlers} />
  ));
}

function Element({ element, handlers }: { element: BlockElement; handlers: Handlers }): ReactNode {
  if (!isKnown(element)) {
    warn(`unknown element type ${JSON.stringify(element.type)}: skipped`);
    return null;
  }
  switch (element.type) {
    case "card":
      return (
        <Card>
          <VStack gap={2}>
            {element.title ? <Heading level={3}>{element.title}</Heading> : null}
            <Children elements={element.children} handlers={handlers} />
          </VStack>
        </Card>
      );
    case "stack":
      return (
        <VStack gap={STACK_GAP[element.gap ?? 2]}>
          <Children elements={element.children} handlers={handlers} />
        </VStack>
      );
    case "row":
      return (
        <HStack
          gap={STACK_GAP[element.gap ?? 2]}
          hAlign={element.align ?? "start"}
          vAlign="center"
          wrap="wrap"
        >
          <Children elements={element.children} handlers={handlers} />
        </HStack>
      );
    case "text":
      return (
        <Text
          type={element.variant ?? "body"}
          weight={element.weight === "semibold" ? "semibold" : undefined}
        >
          {element.value}
        </Text>
      );
    case "heading":
      return <Heading level={element.level ?? 3}>{element.value}</Heading>;
    case "badge":
      return <Badge label={element.value} variant={element.tone ?? "neutral"} />;
    case "link":
      // The schema allows https only; checked again here since trees come from apps.
      if (!element.href.startsWith("https://")) return null;
      return (
        <Link href={element.href} isExternalLink={element.external ?? false} isStandalone>
          {element.label}
        </Link>
      );
    case "button": {
      const variant = element.variant ?? "secondary";
      if (element.navigate !== undefined) {
        const path = element.navigate;
        return (
          <Button
            label={element.label}
            variant={variant}
            onClick={() => handlers.onNavigate?.(path)}
          />
        );
      }
      const action = element.action;
      return (
        <Button
          label={element.label}
          variant={variant}
          isDisabled={!handlers.onAction}
          clickAction={async () => {
            await handlers.onAction?.(action);
          }}
        />
      );
    }
    case "divider":
      return <Divider />;
    case "image":
      if (!element.src.startsWith("https://")) return null;
      return (
        <img
          src={element.src}
          alt={element.alt}
          width={element.width}
          height={element.height}
          loading="lazy"
          style={{ maxWidth: "100%", height: "auto" }}
        />
      );
    case "list":
      return (
        <List hasDividers>
          {element.items.map((item, place) => (
            <ListItem
              key={`${place}:${item.label}`}
              label={item.label}
              description={item.description}
            />
          ))}
        </List>
      );
    case "table":
      return <BlockTable element={element} />;
    case "keyvalue":
      return (
        <MetadataList>
          {element.items.map((item, place) => (
            <MetadataListItem key={`${place}:${item.label}`} label={item.label}>
              {item.value}
            </MetadataListItem>
          ))}
        </MetadataList>
      );
    case "form":
      return <BlockForm form={element} onAction={handlers.onAction} />;
    case "banner":
      return (
        <Banner status={element.tone} title={element.title} description={element.description} />
      );
  }
}

type Row = Record<string, unknown> & { __row: number };

function BlockTable({ element }: { element: TableElement }) {
  const columns: TableColumn<Row>[] = element.columns.map((column) => ({
    key: column.key,
    header: column.header,
    align: column.align === "end" ? "end" : "start",
    renderCell: (row: Row) => {
      const cell = row[column.key];
      return typeof cell === "string" || typeof cell === "number" ? String(cell) : "—";
    },
  }));
  const data: Row[] = element.rows.map((row, place) => ({ ...row, __row: place }));
  return <Table data={data} columns={columns} idKey="__row" />;
}

function initialValues(fields: FieldElement[]): FormValues {
  const values: FormValues = {};
  for (const field of fields) {
    if (field.input === "checkbox") values[field.name] = field.value ?? false;
    else if (field.input === "number") values[field.name] = field.value ?? null;
    else values[field.name] = field.value ?? "";
  }
  return values;
}

function BlockForm({ form, onAction }: { form: FormElement; onAction: Handlers["onAction"] }) {
  const [values, setValues] = useState(() => initialValues(form.fields));
  const [pending, startTransition] = useTransition();
  const set = (name: string, value: FormValues[string]) =>
    setValues((current) => ({ ...current, [name]: value }));

  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (!onAction) return;
    startTransition(async () => {
      await onAction(form.action, values);
    });
  };

  return (
    <form onSubmit={submit}>
      <VStack gap={3}>
        {form.fields.map((field) => (
          <FormField
            key={field.name}
            field={field}
            value={values[field.name] ?? null}
            onChange={set}
          />
        ))}
        <HStack hAlign="end">
          <Button
            type="submit"
            variant="primary"
            label={form.submit_label ?? "Save"}
            isLoading={pending}
            isDisabled={!onAction}
          />
        </HStack>
      </VStack>
    </form>
  );
}

function FormField({
  field,
  value,
  onChange,
}: {
  field: FieldElement;
  value: FormValues[string];
  onChange: (name: string, value: FormValues[string]) => void;
}) {
  const common = {
    label: field.label,
    description: field.description,
    isRequired: field.required ?? false,
    htmlName: field.name,
  };
  switch (field.input) {
    case "text":
      return (
        <TextInput
          {...common}
          placeholder={field.placeholder}
          value={typeof value === "string" ? value : ""}
          onChange={(next) => onChange(field.name, next)}
        />
      );
    case "number":
      return (
        <NumberInput
          {...common}
          min={field.min}
          max={field.max}
          value={typeof value === "number" ? value : null}
          onChange={(next: number | null) => onChange(field.name, next)}
        />
      );
    case "select":
      return (
        <Selector
          {...common}
          options={field.options}
          value={typeof value === "string" && value !== "" ? value : undefined}
          onChange={(next: string) => onChange(field.name, next)}
        />
      );
    case "checkbox":
      return (
        <CheckboxInput
          label={common.label}
          description={common.description}
          isRequired={common.isRequired}
          htmlName={common.htmlName}
          value={value === true}
          onChange={(checked) => onChange(field.name, checked)}
        />
      );
  }
}
