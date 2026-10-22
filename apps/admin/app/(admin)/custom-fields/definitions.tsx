"use client";

import { AlertDialog } from "@astryxdesign/core/AlertDialog";
import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { CheckboxInput } from "@astryxdesign/core/CheckboxInput";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { IconButton } from "@astryxdesign/core/IconButton";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { MultiSelector } from "@astryxdesign/core/MultiSelector";
import { Selector } from "@astryxdesign/core/Selector";
import {
  Table,
  TableBody,
  TableCell,
  TableHeader,
  TableHeaderCell,
  TableRow,
} from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";
import { TextArea } from "@astryxdesign/core/TextArea";
import { TextInput } from "@astryxdesign/core/TextInput";
import {
  ArrowDownIcon,
  ArrowUpIcon,
  PencilSquareIcon,
  PlusIcon,
  TrashIcon,
} from "@heroicons/react/24/outline";
import { useRef, useState, useTransition } from "react";

import { useCurrencies } from "@/components/currencies";
import {
  BASE_TYPES,
  type BaseType,
  DIMENSIONS,
  type Definition,
  OWNER_TYPES,
  type OwnerType,
  REFERENCE_KINDS,
  URL_SCHEMES,
  type Validations,
  allowsPersonal,
  isListable,
  managerLabel,
  mayBeFilterable,
  mayBeUnique,
  jsonObject,
  parseType,
  scalarText,
  typeLabel,
  unitsOf,
  validationsOf,
} from "@/lib/custom-fields";
import { currencyCodes } from "@/lib/format";
import { type Suggestion, suggestionBody } from "@/lib/well-known-fields";

import {
  type CreateRequest,
  createDefinition,
  deleteDefinition,
  moveDefinition,
  updateDefinition,
} from "./actions";

export function DefinitionsCard({
  owner,
  definitions,
  suggestions,
  canWrite,
}: {
  owner: OwnerType;
  /** The owner type's definitions, in their order. */
  definitions: Definition[];
  /** The well-known fields the merchant doesn't have yet. */
  suggestions: Suggestion[];
  /** `custom_fields:write`: the merchant's fields can change. */
  canWrite: boolean;
}) {
  const [editing, setEditing] = useState<Definition | "new">();
  const [deleting, setDeleting] = useState<Definition>();
  const [error, setError] = useState<string>();
  const [moving, setMoving] = useState<string>();
  const [, startTransition] = useTransition();
  const ownerLabel = OWNER_TYPES.find((o) => o.value === owner)?.label ?? owner;

  const move = (definition: Definition, position: number) => {
    setMoving(definition.id);
    startTransition(async () => {
      const result = await moveDefinition(definition.id, position);
      setError(result.error);
      setMoving(undefined);
    });
  };

  return (
    <VStack gap={4}>
      <Card>
        <VStack gap={3}>
          <HStack gap={2} hAlign="between" vAlign="center" wrap="wrap">
            <VStack gap={1}>
              <Text type="display-3" as="h2">
                {ownerLabel}
              </Text>
              <Text type="supporting" color="secondary">
                Shown on each of their pages in this order.
                {!canWrite && " Changing fields needs the custom_fields:write scope."}
              </Text>
            </VStack>
            {canWrite && (
              <Button
                label="Add field"
                variant="secondary"
                icon={<PlusIcon />}
                onClick={() => setEditing("new")}
              />
            )}
          </HStack>
          {error && <Banner status="error" title={error} />}
          {definitions.length === 0 ? (
            <Text color="secondary">No fields yet.</Text>
          ) : (
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHeaderCell>Field</TableHeaderCell>
                  <TableHeaderCell>Type</TableHeaderCell>
                  <TableHeaderCell>Uses</TableHeaderCell>
                  <TableHeaderCell>
                    <span className="visually-hidden">Actions</span>
                  </TableHeaderCell>
                </TableRow>
              </TableHeader>
              <TableBody>
                {definitions.map((definition, i) => {
                  const manager = managerLabel(definition);
                  const isOwn = !definition.managed_by;
                  return (
                    <TableRow key={definition.id}>
                      <TableCell>
                        <VStack gap={0.5}>
                          <Text weight="medium">{definition.name}</Text>
                          <Text type="supporting" color="secondary">
                            {definition.field}
                          </Text>
                        </VStack>
                      </TableCell>
                      <TableCell>
                        <Text>{typeLabel(definition.type)}</Text>
                      </TableCell>
                      <TableCell>
                        <HStack gap={1} wrap="wrap">
                          {manager && <Badge label={manager} variant="info" />}
                          {definition.access.storefront === "read" && (
                            <Badge label="Storefront" variant="success" />
                          )}
                          {definition.capabilities.filterable && <Badge label="Filterable" />}
                          {definition.capabilities.unique && <Badge label="Unique" />}
                          {definition.personal && <Badge label="Personal" variant="warning" />}
                          {definition.private && <Badge label="Private to its app" />}
                          {!isOwn && definition.access.admin === "read" && (
                            <Badge label="Read-only for staff" />
                          )}
                        </HStack>
                      </TableCell>
                      <TableCell>
                        {isOwn && canWrite && (
                          <HStack gap={1} hAlign="end">
                            <IconButton
                              label={`Move ${definition.name} up`}
                              icon={<ArrowUpIcon />}
                              size="sm"
                              variant="ghost"
                              isDisabled={i === 0 || moving !== undefined}
                              onClick={() => move(definition, definition.position - 1)}
                            />
                            <IconButton
                              label={`Move ${definition.name} down`}
                              icon={<ArrowDownIcon />}
                              size="sm"
                              variant="ghost"
                              isDisabled={i === definitions.length - 1 || moving !== undefined}
                              onClick={() => move(definition, definition.position + 1)}
                            />
                            <IconButton
                              label={`Edit ${definition.name}`}
                              icon={<PencilSquareIcon />}
                              size="sm"
                              variant="ghost"
                              onClick={() => setEditing(definition)}
                            />
                            <IconButton
                              label={`Delete ${definition.name}`}
                              icon={<TrashIcon />}
                              size="sm"
                              variant="ghost"
                              onClick={() => setDeleting(definition)}
                            />
                          </HStack>
                        )}
                      </TableCell>
                    </TableRow>
                  );
                })}
              </TableBody>
            </Table>
          )}
        </VStack>
      </Card>
      {canWrite && suggestions.length > 0 && (
        <SuggestionsCard suggestions={suggestions} onError={setError} />
      )}
      {editing && (
        <DefinitionDialog
          owner={owner}
          definition={editing === "new" ? undefined : editing}
          onClose={() => setEditing(undefined)}
        />
      )}
      {deleting && <DeleteDialog definition={deleting} onClose={() => setDeleting(undefined)} />}
    </VStack>
  );
}

/**
 * The well-known fields worth having, each created with one click from the body the
 * API suggests. Once created they're ordinary fields above.
 */
function SuggestionsCard({
  suggestions,
  onError,
}: {
  suggestions: Suggestion[];
  onError: (error: string | undefined) => void;
}) {
  const [adding, setAdding] = useState<string>();
  const [, startTransition] = useTransition();
  const add = (suggestion: Suggestion) => {
    setAdding(suggestion.key);
    startTransition(async () => {
      const result = await createDefinition(suggestionBody(suggestion));
      onError(result.error);
      setAdding(undefined);
    });
  };
  return (
    <Card>
      <VStack gap={3}>
        <VStack gap={1}>
          <Text type="display-3" as="h2">
            Suggested
          </Text>
          <Text type="supporting" color="secondary">
            Fields Hypha knows what to do with: the admin shows them in place, and apps expect them
            by these names. Add the ones you want; change or delete them like any other.
          </Text>
        </VStack>
        <VStack gap={3}>
          {suggestions.map((suggestion) => (
            <HStack key={suggestion.key} gap={3} hAlign="between" vAlign="center" wrap="wrap">
              <VStack gap={0.5}>
                <HStack gap={1} vAlign="center" wrap="wrap">
                  <Text weight="medium">{suggestion.name}</Text>
                  <Text type="supporting" color="secondary">
                    {suggestion.field} · {typeLabel(suggestion.type)}
                  </Text>
                  {suggestion.personal && <Badge label="Personal" variant="warning" />}
                </HStack>
                <Text type="supporting" color="secondary">
                  {suggestion.why}
                </Text>
              </VStack>
              <Button
                label="Add"
                variant="secondary"
                size="sm"
                icon={<PlusIcon />}
                isLoading={adding === suggestion.key}
                isDisabled={adding !== undefined && adding !== suggestion.key}
                onClick={() => add(suggestion)}
              />
            </HStack>
          ))}
        </VStack>
      </VStack>
    </Card>
  );
}

function DeleteDialog({ definition, onClose }: { definition: Definition; onClose: () => void }) {
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  return (
    <AlertDialog
      isOpen
      onOpenChange={(isOpen) => !isOpen && onClose()}
      title={`Delete ${definition.name}?`}
      description={
        error ??
        `Every value of ${definition.field} goes with it, on every record, and it can't be undone. Filters and integrations using it stop working.`
      }
      actionLabel="Delete field"
      isActionLoading={isPending}
      onAction={() =>
        startTransition(async () => {
          const result = await deleteDefinition(definition.id);
          if (result.error) setError(result.error);
          else onClose();
        })
      }
    />
  );
}

// ---------- the definition form ----------

type Choice = { key: string; label: string };

/** The form as typed: every validation as text, read into the API's shape on save. */
type Form = {
  key: string;
  name: string;
  description: string;
  base: BaseType;
  isList: boolean;
  min_length: string;
  max_length: string;
  pattern: string;
  min: string;
  max: string;
  max_scale: string;
  schemes: string[];
  domains: string;
  currencies: string[];
  dimension: string;
  choices: Choice[];
  kind: string;
  min_items: string;
  max_items: string;
  unique_items: boolean;
  storefront: "none" | "read";
  personal: boolean;
  filterable: boolean;
  unique: boolean;
};

/** A bound as the form shows it: measurements as "1.5 kg", the rest as written. */
function boundText(value: unknown): string {
  if (typeof value === "object" && value !== null && "value" in value && "unit" in value) {
    return `${scalarText(value.value)} ${scalarText(value.unit)}`;
  }
  return scalarText(value);
}

/** A count as the form shows it; "" for none. */
const countText = (n: number | null | undefined) =>
  n === undefined || n === null ? "" : String(n);

function formOf(definition?: Definition): Form {
  const v: Validations = definition?.validations ?? {};
  const { base, isList } = parseType(definition?.type ?? "text");
  return {
    key: definition?.key ?? "",
    name: definition?.name ?? "",
    description: definition?.description ?? "",
    base,
    isList,
    min_length: countText(v.min_length),
    max_length: countText(v.max_length),
    pattern: v.pattern ?? "",
    min: boundText(v.min),
    max: boundText(v.max),
    max_scale: countText(v.max_scale),
    schemes: v.schemes ?? [],
    domains: (v.domains ?? []).join(", "),
    currencies: v.currencies ?? [],
    dimension: v.dimension ?? "weight",
    choices: v.choices ?? [],
    kind: v.kind ?? "product",
    min_items: countText(v.min_items),
    max_items: countText(v.max_items),
    unique_items: v.unique_items ?? false,
    storefront: definition?.access.storefront === "read" ? "read" : "none",
    personal: definition?.personal ?? false,
    filterable: definition?.capabilities.filterable ?? false,
    unique: definition?.capabilities.unique ?? false,
  };
}

const typeOf = (form: Form) => (form.isList ? `list.${form.base}` : form.base);

/** A whole number from the form, `undefined` for blank, or an error. */
function count(text: string, what: string): number | undefined | { error: string } {
  const t = text.trim();
  if (t === "") return undefined;
  return /^\d+$/.test(t) ? Number(t) : { error: `${what} is a whole number.` };
}

/** A `min`/`max` in the type's own terms. */
type Bound = number | string | { value: string; unit: string };

function bound(form: Form, text: string, what: string): Bound | undefined | { error: string } {
  const t = text.trim();
  if (t === "") return undefined;
  switch (form.base) {
    case "integer":
    case "money":
      return /^-?\d+$/.test(t)
        ? Number(t)
        : {
            error:
              form.base === "money"
                ? `${what} is a whole number of minor units (cents).`
                : `${what} is a whole number.`,
          };
    case "measurement": {
      const match = /^(-?\d+(?:\.\d+)?)\s*([a-z_]+)$/.exec(t);
      return match
        ? { value: match[1], unit: match[2] }
        : { error: `${what} is a number with its unit, like 1.5 ${unitsOf(form.dimension)[0]}.` };
    }
    default:
      return t;
  }
}

const isError = (v: unknown): v is { error: string } =>
  typeof v === "object" && v !== null && "error" in v;

/** The form's whole-number rules, with what each is called in a message. */
const COUNTS = [
  { name: "min_length", what: "The minimum length", read: (f: Form) => f.min_length },
  { name: "max_length", what: "The maximum length", read: (f: Form) => f.max_length },
  { name: "max_scale", what: "The number of decimals", read: (f: Form) => f.max_scale },
  { name: "min_items", what: "The minimum number of items", read: (f: Form) => f.min_items },
  { name: "max_items", what: "The maximum number of items", read: (f: Form) => f.max_items },
] as const;

/** The validations the form's type takes, or what to fix. */
function validationsFrom(form: Form): Validations | { error: string } {
  const allowed = new Set(validationsOf(typeOf(form)));
  const out: Validations = {};
  for (const { name, what, read } of COUNTS) {
    if (!allowed.has(name)) continue;
    const n = count(read(form), what);
    if (isError(n)) return n;
    if (n !== undefined) out[name] = n;
  }
  for (const [name, what] of [
    ["min", "The minimum"],
    ["max", "The maximum"],
  ] as const) {
    if (!allowed.has(name)) continue;
    const b = bound(form, form[name], what);
    if (isError(b)) return b;
    if (b !== undefined) out[name] = jsonObject(b);
  }
  if (allowed.has("pattern") && form.pattern.trim()) out.pattern = form.pattern.trim();
  if (allowed.has("schemes") && form.schemes.length > 0) out.schemes = form.schemes;
  if (allowed.has("domains")) {
    const domains = form.domains
      .split(",")
      .map((d) => d.trim())
      .filter(Boolean);
    if (domains.length > 0) out.domains = domains;
  }
  if (allowed.has("currencies") && form.currencies.length > 0) out.currencies = form.currencies;
  if (allowed.has("dimension")) out.dimension = form.dimension;
  if (allowed.has("kind")) out.kind = form.kind;
  if (allowed.has("choices")) {
    const choices = form.choices
      .map((c) => ({ key: c.key.trim(), label: c.label.trim() }))
      .filter((c) => c.key || c.label);
    if (choices.length === 0) return { error: "Add at least one choice." };
    if (choices.some((c) => !c.key || !c.label)) {
      return { error: "Give every choice a key and a label." };
    }
    out.choices = choices;
  }
  if (allowed.has("unique_items") && form.unique_items) out.unique_items = true;
  return out;
}

/** A key from a name: "Care instructions" → "care_instructions". */
function keyFrom(name: string): string {
  return name
    .toLowerCase()
    .normalize("NFKD")
    .replace(/[^a-z0-9]+/g, "_")
    .replace(/^[^a-z]+|_+$/g, "")
    .slice(0, 64);
}

/** Adds a field in `custom`, or changes one: its key and type can't change. */
function DefinitionDialog({
  owner,
  definition,
  onClose,
}: {
  owner: OwnerType;
  definition?: Definition;
  onClose: () => void;
}) {
  const currencies = useCurrencies();
  const [form, setForm] = useState(() => formOf(definition));
  // Until the key is typed in, it follows the name.
  const isKeyTouched = useRef(definition !== undefined);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const set = (change: Partial<Form>) => {
    setForm((before) => ({ ...before, ...change }));
    setError(undefined);
  };
  const type = typeOf(form);
  const allowed = new Set(validationsOf(type));
  const isNew = definition === undefined;

  const submit = () => {
    if (!form.name.trim()) return setError("Give the field a name.");
    if (isNew && !/^[a-z][a-z0-9_]{1,63}$/.test(form.key)) {
      return setError(
        "The key is 2 to 64 lowercase letters, digits and _, starting with a letter.",
      );
    }
    const validations = validationsFrom(form);
    if (isError(validations)) return setError(validations.error);
    const capabilities = {
      filterable: mayBeFilterable(type) && form.filterable,
      unique: mayBeUnique(type) && form.unique,
    };
    const access = { admin: "read_write" as const, storefront: form.storefront };
    const personal = allowsPersonal(owner) && form.personal;
    startTransition(async () => {
      const body: CreateRequest = {
        owner_type: owner,
        key: form.key,
        type,
        name: form.name.trim(),
        description: form.description.trim() || null,
        validations,
        access,
        personal,
        capabilities,
      };
      const result = definition
        ? await updateDefinition(definition.id, {
            name: body.name,
            description: body.description,
            validations,
            access,
            personal,
            capabilities,
          })
        : await createDefinition(body);
      if (result.error) setError(result.error);
      else onClose();
    });
  };

  const typeOptions = BASE_TYPES.map((t) => ({ value: t.value, label: t.label }));

  return (
    <Dialog isOpen onOpenChange={(isOpen) => !isOpen && onClose()} width={640} purpose="form">
      <DialogHeader
        title={isNew ? "Add field" : `Edit ${definition.name}`}
        subtitle={
          isNew
            ? "Its key and type can't change later."
            : `${definition.field} · ${typeLabel(definition.type)}. Its key and type can't change.`
        }
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      {/* The fields scroll on short screens; the buttons stay in view below them. A flex
          item keeps its content's height unless allowed to shrink (`minHeight: 0`). */}
      <div style={{ flex: "1 1 auto", minHeight: 0, overflowY: "auto" }}>
        <VStack gap={4} padding={4}>
          <TextInput
            label="Name"
            description="What staff see on the form."
            value={form.name}
            onChange={(name) =>
              set({ name, ...(isKeyTouched.current ? {} : { key: keyFrom(name) }) })
            }
          />
          {isNew && (
            <TextInput
              label="Key"
              description={`Code reads it as custom.${form.key || "key"}: lowercase letters, digits and _.`}
              value={form.key}
              onChange={(key) => {
                isKeyTouched.current = true;
                set({ key });
              }}
            />
          )}
          <TextArea
            label="Description"
            isOptional
            description="Help shown under the field."
            value={form.description}
            onChange={(description) => set({ description })}
            rows={2}
          />
          {isNew && (
            <HStack gap={3} vAlign="end" wrap="wrap">
              <Selector
                label="Type"
                options={typeOptions}
                value={form.base}
                onChange={(value: string) => {
                  const base = BASE_TYPES.find((t) => t.value === value)?.value ?? "text";
                  set({ base, isList: form.isList && isListable(base) });
                }}
                width={240}
              />
              <CheckboxInput
                label="A list of values"
                value={form.isList}
                isDisabled={!isListable(form.base)}
                onChange={(isList) => set({ isList })}
              />
            </HStack>
          )}
          {isNew && (
            <Text type="supporting" color="secondary">
              {BASE_TYPES.find((t) => t.value === form.base)?.description}
            </Text>
          )}

          <ValidationsEditor
            form={form}
            allowed={allowed}
            set={set}
            currencyList={currencyCodes(currencies)}
          />

          <VStack gap={2}>
            <Text weight="medium">Access and uses</Text>
            <Selector
              label="Storefront"
              description="Whether storefronts can read the value."
              options={[
                { value: "none", label: "Hidden from storefronts" },
                { value: "read", label: "Storefronts can read it" },
              ]}
              value={form.storefront}
              onChange={(value: string) => set({ storefront: value === "read" ? "read" : "none" })}
              width={280}
            />
            {mayBeFilterable(type) && (
              <CheckboxInput
                label="Filterable"
                description="Offered as a filter on the list, and usable as a search filter."
                value={form.filterable}
                onChange={(filterable) => set({ filterable })}
              />
            )}
            {mayBeUnique(type) && (
              <CheckboxInput
                label="Unique"
                description="No two records hold the same value (text ignoring case)."
                value={form.unique}
                onChange={(unique) => set({ unique })}
              />
            )}
            {allowsPersonal(owner) && (
              <CheckboxInput
                label="Personal data"
                description="About a person (a birthday, a gift message): hidden from apps and keys without customer_data:profile."
                value={form.personal}
                onChange={(personal) => set({ personal })}
              />
            )}
          </VStack>
        </VStack>
      </div>
      <VStack gap={3} padding={4}>
        {error && <Banner status="error" title={error} />}
        <HStack gap={2} hAlign="end">
          <Button label="Cancel" variant="ghost" onClick={onClose} />
          <Button
            label={isNew ? "Add field" : "Save"}
            variant="primary"
            isLoading={isPending}
            onClick={submit}
          />
        </HStack>
      </VStack>
    </Dialog>
  );
}

/** The rules a type's values follow, only those it takes. */
function ValidationsEditor({
  form,
  allowed,
  set,
  currencyList,
}: {
  form: Form;
  allowed: Set<keyof Validations>;
  set: (change: Partial<Form>) => void;
  currencyList: string[];
}) {
  if (allowed.size === 0) return null;
  const boundHint: Partial<Record<BaseType, string>> = {
    integer: "A whole number.",
    decimal: "Like 12.50.",
    date: "Like 2026-10-20.",
    date_time: "RFC 3339, like 2026-10-20T09:30:00Z.",
    money: "In minor units (cents), in the value's currency.",
    measurement: `With its unit, like 1.5 ${unitsOf(form.dimension)[0]}.`,
  };
  return (
    <VStack gap={3}>
      <Text weight="medium">Rules</Text>
      {(allowed.has("min_length") || allowed.has("max_length")) && (
        <HStack gap={3} wrap="wrap">
          {allowed.has("min_length") && (
            <TextInput
              label="Minimum length"
              isOptional
              value={form.min_length}
              onChange={(min_length) => set({ min_length })}
              width={160}
            />
          )}
          {allowed.has("max_length") && (
            <TextInput
              label="Maximum length"
              isOptional
              value={form.max_length}
              onChange={(max_length) => set({ max_length })}
              width={160}
            />
          )}
        </HStack>
      )}
      {allowed.has("pattern") && (
        <TextInput
          label="Pattern"
          isOptional
          description="A regular expression the whole value matches, like [A-Z]{3}-\d+ (no look-around or back-references)."
          value={form.pattern}
          onChange={(pattern) => set({ pattern })}
        />
      )}
      {allowed.has("dimension") && (
        <Selector
          label="Measures"
          description="Required. Values take any unit of it."
          options={DIMENSIONS}
          value={form.dimension}
          onChange={(dimension: string) => set({ dimension })}
          width={200}
        />
      )}
      {(allowed.has("min") || allowed.has("max")) && (
        <HStack gap={3} wrap="wrap">
          <TextInput
            label="Minimum"
            isOptional
            description={boundHint[form.base]}
            value={form.min}
            onChange={(min) => set({ min })}
            width={220}
          />
          <TextInput
            label="Maximum"
            isOptional
            description={boundHint[form.base]}
            value={form.max}
            onChange={(max) => set({ max })}
            width={220}
          />
        </HStack>
      )}
      {allowed.has("max_scale") && (
        <TextInput
          label="Decimals at most"
          isOptional
          value={form.max_scale}
          onChange={(max_scale) => set({ max_scale })}
          width={160}
        />
      )}
      {allowed.has("schemes") && (
        <MultiSelector
          label="Allowed schemes"
          isOptional
          description="Any of them when none is chosen."
          options={URL_SCHEMES.map((s) => ({ value: s, label: s }))}
          value={form.schemes}
          onChange={(schemes) => set({ schemes })}
        />
      )}
      {allowed.has("domains") && (
        <TextInput
          label="Allowed domains"
          isOptional
          description="Comma-separated, like example.com, shop.example.com. Any when empty."
          value={form.domains}
          onChange={(domains) => set({ domains })}
        />
      )}
      {allowed.has("currencies") && (
        <MultiSelector
          label="Currencies"
          isOptional
          description="Any currency when none is chosen."
          options={currencyList.map((c) => ({ value: c, label: c }))}
          value={form.currencies}
          hasSearch
          onChange={(currencies) => set({ currencies })}
        />
      )}
      {allowed.has("kind") && (
        <Selector
          label="Refers to"
          description="Required: what the value points at."
          options={REFERENCE_KINDS.map((k) => ({ value: k.value, label: k.label }))}
          value={form.kind}
          onChange={(kind: string) => set({ kind })}
          width={200}
        />
      )}
      {allowed.has("choices") && (
        <ChoicesEditor choices={form.choices} onChange={(choices) => set({ choices })} />
      )}
      {(allowed.has("min_items") || allowed.has("max_items")) && (
        <HStack gap={3} wrap="wrap" vAlign="end">
          <TextInput
            label="Fewest items"
            isOptional
            value={form.min_items}
            onChange={(min_items) => set({ min_items })}
            width={140}
          />
          <TextInput
            label="Most items"
            isOptional
            description="Up to 128."
            value={form.max_items}
            onChange={(max_items) => set({ max_items })}
            width={140}
          />
          <CheckboxInput
            label="No item twice"
            value={form.unique_items}
            onChange={(unique_items) => set({ unique_items })}
          />
        </HStack>
      )}
    </VStack>
  );
}

/**
 * A choice field's choices: the key is stored, filtered and faceted, so it's fixed once
 * saved; the label can change.
 */
function ChoicesEditor({
  choices,
  onChange,
}: {
  choices: Choice[];
  onChange: (choices: Choice[]) => void;
}) {
  const [rows, setRows] = useState(() =>
    (choices.length > 0 ? choices : [{ key: "", label: "" }]).map((c, i) => ({
      ...c,
      id: i,
      isSaved: c.key !== "",
    })),
  );
  const next = useRef(rows.length);
  const update = (nextRows: typeof rows) => {
    setRows(nextRows);
    onChange(nextRows.map(({ key, label }) => ({ key, label })));
  };
  return (
    <VStack gap={2}>
      <Text weight="medium">Choices</Text>
      <Text type="supporting" color="secondary">
        Keys are lowercase letters, digits, _ and -; they&apos;re what&apos;s stored, so they
        can&apos;t change once saved. Labels are what staff and shoppers see.
      </Text>
      {rows.map((row) => (
        <HStack key={row.id} gap={2} vAlign="end">
          <TextInput
            label="Label"
            isLabelHidden
            placeholder="Label"
            value={row.label}
            onChange={(label) =>
              update(
                rows.map((r) =>
                  r.id === row.id
                    ? {
                        ...r,
                        label,
                        key: r.isSaved || r.key !== keyOfLabel(r.label) ? r.key : keyOfLabel(label),
                      }
                    : r,
                ),
              )
            }
          />
          <TextInput
            label="Key"
            isLabelHidden
            placeholder="key"
            value={row.key}
            isDisabled={row.isSaved}
            onChange={(key) => update(rows.map((r) => (r.id === row.id ? { ...r, key } : r)))}
            width={180}
          />
          <IconButton
            label={`Remove ${row.label || "choice"}`}
            icon={<TrashIcon />}
            size="sm"
            variant="ghost"
            isDisabled={rows.length === 1}
            onClick={() => update(rows.filter((r) => r.id !== row.id))}
          />
        </HStack>
      ))}
      <HStack>
        <Button
          label="Add choice"
          size="sm"
          variant="secondary"
          icon={<PlusIcon />}
          onClick={() => {
            update([...rows, { key: "", label: "", id: next.current, isSaved: false }]);
            next.current += 1;
          }}
        />
      </HStack>
    </VStack>
  );
}

/** "Dry skin" → "dry-skin". */
function keyOfLabel(label: string): string {
  return label
    .toLowerCase()
    .normalize("NFKD")
    .replace(/[^a-z0-9_-]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 64);
}
