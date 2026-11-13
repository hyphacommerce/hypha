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
  type FieldsT,
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
  ownerOf,
  parseType,
  scalarText,
  typeLabel,
  unitsOf,
  validationsOf,
} from "@/lib/custom-fields";
import { currencyCodes } from "@/lib/format";
import { useT } from "@/lib/use-t";
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
  const t = useT<"custom_fields">();
  const [editing, setEditing] = useState<Definition | "new">();
  const [deleting, setDeleting] = useState<Definition>();
  const [error, setError] = useState<string>();
  const [moving, setMoving] = useState<string>();
  const [, startTransition] = useTransition();
  const ownerLabel = t(ownerOf(owner).label);

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
                {canWrite
                  ? t("custom_fields.list_order_hint")
                  : t("custom_fields.list_order_hint_read_only")}
              </Text>
            </VStack>
            {canWrite && (
              <Button
                label={t("custom_fields.add_field")}
                variant="secondary"
                icon={<PlusIcon />}
                onClick={() => setEditing("new")}
              />
            )}
          </HStack>
          {error && <Banner status="error" title={error} />}
          {definitions.length === 0 ? (
            <Text color="secondary">{t("custom_fields.list_empty")}</Text>
          ) : (
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHeaderCell>{t("custom_fields.list_column_field")}</TableHeaderCell>
                  <TableHeaderCell>{t("common.type")}</TableHeaderCell>
                  <TableHeaderCell>{t("custom_fields.list_column_uses")}</TableHeaderCell>
                  <TableHeaderCell>
                    <span className="visually-hidden">{t("common.actions")}</span>
                  </TableHeaderCell>
                </TableRow>
              </TableHeader>
              <TableBody>
                {definitions.map((definition, i) => {
                  const manager = managerLabel(t, definition);
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
                        <Text>{typeLabel(t, definition.type)}</Text>
                      </TableCell>
                      <TableCell>
                        <HStack gap={1} wrap="wrap">
                          {manager && <Badge label={manager} variant="info" />}
                          {definition.access.storefront === "read" && (
                            <Badge label={t("custom_fields.storefront")} variant="success" />
                          )}
                          {definition.capabilities.filterable && (
                            <Badge label={t("custom_fields.filterable")} />
                          )}
                          {definition.capabilities.unique && (
                            <Badge label={t("custom_fields.unique")} />
                          )}
                          {definition.personal && (
                            <Badge label={t("custom_fields.badge_personal")} variant="warning" />
                          )}
                          {definition.private && <Badge label={t("custom_fields.badge_private")} />}
                          {!isOwn && definition.access.admin === "read" && (
                            <Badge label={t("custom_fields.badge_staff_read_only")} />
                          )}
                        </HStack>
                      </TableCell>
                      <TableCell>
                        {isOwn && canWrite && (
                          <HStack gap={1} hAlign="end">
                            <IconButton
                              label={t("custom_fields.move_up", { name: definition.name })}
                              icon={<ArrowUpIcon />}
                              size="sm"
                              variant="ghost"
                              isDisabled={i === 0 || moving !== undefined}
                              onClick={() => move(definition, definition.position - 1)}
                            />
                            <IconButton
                              label={t("custom_fields.move_down", { name: definition.name })}
                              icon={<ArrowDownIcon />}
                              size="sm"
                              variant="ghost"
                              isDisabled={i === definitions.length - 1 || moving !== undefined}
                              onClick={() => move(definition, definition.position + 1)}
                            />
                            <IconButton
                              label={t("custom_fields.edit_named", { name: definition.name })}
                              icon={<PencilSquareIcon />}
                              size="sm"
                              variant="ghost"
                              onClick={() => setEditing(definition)}
                            />
                            <IconButton
                              label={t("custom_fields.delete_field_named", {
                                name: definition.name,
                              })}
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
  const t = useT<"custom_fields">();
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
            {t("custom_fields.suggested_title")}
          </Text>
          <Text type="supporting" color="secondary">
            {t("custom_fields.suggested_description")}
          </Text>
        </VStack>
        <VStack gap={3}>
          {suggestions.map((suggestion) => (
            <HStack key={suggestion.key} gap={3} hAlign="between" vAlign="center" wrap="wrap">
              <VStack gap={0.5}>
                <HStack gap={1} vAlign="center" wrap="wrap">
                  <Text weight="medium">{suggestion.name}</Text>
                  <Text type="supporting" color="secondary">
                    {suggestion.field} · {typeLabel(t, suggestion.type)}
                  </Text>
                  {suggestion.personal && (
                    <Badge label={t("custom_fields.badge_personal")} variant="warning" />
                  )}
                </HStack>
                <Text type="supporting" color="secondary">
                  {suggestion.why}
                </Text>
              </VStack>
              <Button
                label={t("common.add")}
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
  const t = useT<"custom_fields">();
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  return (
    <AlertDialog
      isOpen
      onOpenChange={(isOpen) => !isOpen && onClose()}
      title={t("common.delete_named", { name: definition.name })}
      description={
        error ?? t("custom_fields.delete_dialog_description", { field: definition.field })
      }
      actionLabel={t("custom_fields.delete_dialog_action")}
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

/** A whole number from the form, `undefined` for blank, or `null` when it isn't one. */
function count(text: string): number | undefined | null {
  const typed = text.trim();
  if (typed === "") return undefined;
  return /^\d+$/.test(typed) ? Number(typed) : null;
}

/** A `min`/`max` in the type's own terms. */
type Bound = number | string | { value: string; unit: string };

function bound(
  form: Form,
  which: "min" | "max",
  t: FieldsT,
): Bound | undefined | { error: string } {
  const typed = form[which].trim();
  if (typed === "") return undefined;
  switch (form.base) {
    case "integer":
    case "money":
      return /^-?\d+$/.test(typed)
        ? Number(typed)
        : {
            error:
              form.base === "money"
                ? t(`custom_fields.error_${which}_minor_units`)
                : t(`custom_fields.error_${which}_whole`),
          };
    case "measurement": {
      const match = /^(-?\d+(?:\.\d+)?)\s*([a-z_]+)$/.exec(typed);
      return match
        ? { value: match[1], unit: match[2] }
        : {
            error: t(`custom_fields.error_${which}_measurement`, {
              unit: unitsOf(form.dimension)[0],
            }),
          };
    }
    default:
      return typed;
  }
}

const isError = (v: unknown): v is { error: string } =>
  typeof v === "object" && v !== null && "error" in v;

/** The form's whole-number rules; `custom_fields.error_<name>` says what each must be. */
const COUNTS = ["min_length", "max_length", "max_scale", "min_items", "max_items"] as const;

/** The validations the form's type takes, or what to fix. */
function validationsFrom(form: Form, t: FieldsT): Validations | { error: string } {
  const allowed = new Set(validationsOf(typeOf(form)));
  const out: Validations = {};
  for (const name of COUNTS) {
    if (!allowed.has(name)) continue;
    const n = count(form[name]);
    if (n === null) return { error: t(`custom_fields.error_${name}`) };
    if (n !== undefined) out[name] = n;
  }
  for (const name of ["min", "max"] as const) {
    if (!allowed.has(name)) continue;
    const b = bound(form, name, t);
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
    if (choices.length === 0) return { error: t("custom_fields.error_choices_empty") };
    if (choices.some((c) => !c.key || !c.label)) {
      return { error: t("custom_fields.error_choices_incomplete") };
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
  const t = useT<"custom_fields">();
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
    if (!form.name.trim()) return setError(t("custom_fields.error_name"));
    if (isNew && !/^[a-z][a-z0-9_]{1,63}$/.test(form.key)) {
      return setError(t("custom_fields.error_key"));
    }
    const validations = validationsFrom(form, t);
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

  const typeOptions = BASE_TYPES.map((base) => ({
    value: base,
    label: t(`custom_fields.type_${base}`),
  }));

  return (
    <Dialog isOpen onOpenChange={(isOpen) => !isOpen && onClose()} width={640} purpose="form">
      <DialogHeader
        title={
          isNew
            ? t("custom_fields.add_field")
            : t("custom_fields.edit_named", { name: definition.name })
        }
        subtitle={
          isNew
            ? t("custom_fields.dialog_new_subtitle")
            : t("custom_fields.dialog_edit_subtitle", {
                field: definition.field,
                type: typeLabel(t, definition.type),
              })
        }
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      {/* The fields scroll on short screens; the buttons stay in view below them. A flex
          item keeps its content's height unless allowed to shrink (`minHeight: 0`). */}
      <div style={{ flex: "1 1 auto", minHeight: 0, overflowY: "auto" }}>
        <VStack gap={4} padding={4}>
          <TextInput
            label={t("common.name")}
            description={t("custom_fields.dialog_name_hint")}
            value={form.name}
            onChange={(name) =>
              set({ name, ...(isKeyTouched.current ? {} : { key: keyFrom(name) }) })
            }
          />
          {isNew && (
            <TextInput
              label={t("common.key")}
              description={t("custom_fields.dialog_key_hint", {
                key: form.key || t("custom_fields.choices_key_placeholder"),
              })}
              value={form.key}
              onChange={(key) => {
                isKeyTouched.current = true;
                set({ key });
              }}
            />
          )}
          <TextArea
            label={t("common.description")}
            isOptional
            description={t("custom_fields.dialog_description_hint")}
            value={form.description}
            onChange={(description) => set({ description })}
            rows={2}
          />
          {isNew && (
            <HStack gap={3} vAlign="end" wrap="wrap">
              <Selector
                label={t("common.type")}
                options={typeOptions}
                value={form.base}
                onChange={(value: string) => {
                  const base = BASE_TYPES.find((b) => b === value) ?? "text";
                  set({ base, isList: form.isList && isListable(base) });
                }}
                width={240}
              />
              <CheckboxInput
                label={t("custom_fields.dialog_is_list")}
                value={form.isList}
                isDisabled={!isListable(form.base)}
                onChange={(isList) => set({ isList })}
              />
            </HStack>
          )}
          {isNew && (
            <Text type="supporting" color="secondary">
              {t(`custom_fields.type_${form.base}_description`)}
            </Text>
          )}

          <ValidationsEditor
            form={form}
            allowed={allowed}
            set={set}
            currencyList={currencyCodes(currencies)}
          />

          <VStack gap={2}>
            <Text weight="medium">{t("custom_fields.dialog_access_title")}</Text>
            <Selector
              label={t("custom_fields.storefront")}
              description={t("custom_fields.dialog_storefront_hint")}
              options={[
                { value: "none", label: t("custom_fields.storefront_none") },
                { value: "read", label: t("custom_fields.storefront_read") },
              ]}
              value={form.storefront}
              onChange={(value: string) => set({ storefront: value === "read" ? "read" : "none" })}
              width={280}
            />
            {mayBeFilterable(type) && (
              <CheckboxInput
                label={t("custom_fields.filterable")}
                description={t("custom_fields.dialog_filterable_hint")}
                value={form.filterable}
                onChange={(filterable) => set({ filterable })}
              />
            )}
            {mayBeUnique(type) && (
              <CheckboxInput
                label={t("custom_fields.unique")}
                description={t("custom_fields.dialog_unique_hint")}
                value={form.unique}
                onChange={(unique) => set({ unique })}
              />
            )}
            {allowsPersonal(owner) && (
              <CheckboxInput
                label={t("custom_fields.dialog_personal")}
                description={t("custom_fields.dialog_personal_hint")}
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
          <Button label={t("common.cancel")} variant="ghost" onClick={onClose} />
          <Button
            label={isNew ? t("custom_fields.add_field") : t("common.save")}
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
  const t = useT<"custom_fields">();
  if (allowed.size === 0) return null;
  const boundHint: Partial<Record<BaseType, string>> = {
    integer: t("custom_fields.bound_hint_integer"),
    decimal: t("custom_fields.bound_hint_decimal"),
    date: t("custom_fields.bound_hint_date"),
    date_time: t("custom_fields.bound_hint_date_time"),
    money: t("custom_fields.bound_hint_money"),
    measurement: t("custom_fields.bound_hint_measurement", { unit: unitsOf(form.dimension)[0] }),
  };
  return (
    <VStack gap={3}>
      <Text weight="medium">{t("custom_fields.rules_title")}</Text>
      {(allowed.has("min_length") || allowed.has("max_length")) && (
        <HStack gap={3} wrap="wrap">
          {allowed.has("min_length") && (
            <TextInput
              label={t("custom_fields.rule_min_length")}
              isOptional
              value={form.min_length}
              onChange={(min_length) => set({ min_length })}
              width={160}
            />
          )}
          {allowed.has("max_length") && (
            <TextInput
              label={t("custom_fields.rule_max_length")}
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
          label={t("custom_fields.rule_pattern")}
          isOptional
          description={t("custom_fields.rule_pattern_hint")}
          value={form.pattern}
          onChange={(pattern) => set({ pattern })}
        />
      )}
      {allowed.has("dimension") && (
        <Selector
          label={t("custom_fields.rule_dimension")}
          description={t("custom_fields.rule_dimension_hint")}
          options={DIMENSIONS.map((d) => ({ value: d, label: t(`custom_fields.dimension_${d}`) }))}
          value={form.dimension}
          onChange={(dimension: string) => set({ dimension })}
          width={200}
        />
      )}
      {(allowed.has("min") || allowed.has("max")) && (
        <HStack gap={3} wrap="wrap">
          <TextInput
            label={t("custom_fields.rule_min")}
            isOptional
            description={boundHint[form.base]}
            value={form.min}
            onChange={(min) => set({ min })}
            width={220}
          />
          <TextInput
            label={t("custom_fields.rule_max")}
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
          label={t("custom_fields.rule_max_scale")}
          isOptional
          value={form.max_scale}
          onChange={(max_scale) => set({ max_scale })}
          width={160}
        />
      )}
      {allowed.has("schemes") && (
        <MultiSelector
          label={t("custom_fields.rule_schemes")}
          isOptional
          description={t("custom_fields.rule_schemes_hint")}
          options={URL_SCHEMES.map((s) => ({ value: s, label: s }))}
          value={form.schemes}
          onChange={(schemes) => set({ schemes })}
        />
      )}
      {allowed.has("domains") && (
        <TextInput
          label={t("custom_fields.rule_domains")}
          isOptional
          description={t("custom_fields.rule_domains_hint")}
          value={form.domains}
          onChange={(domains) => set({ domains })}
        />
      )}
      {allowed.has("currencies") && (
        <MultiSelector
          label={t("custom_fields.rule_currencies")}
          isOptional
          description={t("custom_fields.rule_currencies_hint")}
          options={currencyList.map((c) => ({ value: c, label: c }))}
          value={form.currencies}
          hasSearch
          onChange={(currencies) => set({ currencies })}
        />
      )}
      {allowed.has("kind") && (
        <Selector
          label={t("custom_fields.rule_kind")}
          description={t("custom_fields.rule_kind_hint")}
          options={REFERENCE_KINDS.map((k) => ({ value: k.value, label: t(k.label) }))}
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
            label={t("custom_fields.rule_min_items")}
            isOptional
            value={form.min_items}
            onChange={(min_items) => set({ min_items })}
            width={140}
          />
          <TextInput
            label={t("custom_fields.rule_max_items")}
            isOptional
            description={t("custom_fields.rule_max_items_hint")}
            value={form.max_items}
            onChange={(max_items) => set({ max_items })}
            width={140}
          />
          <CheckboxInput
            label={t("custom_fields.rule_unique_items")}
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
  const t = useT<"custom_fields">();
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
      <Text weight="medium">{t("custom_fields.choices_title")}</Text>
      <Text type="supporting" color="secondary">
        {t("custom_fields.choices_hint")}
      </Text>
      {rows.map((row) => (
        <HStack key={row.id} gap={2} vAlign="end">
          <TextInput
            label={t("custom_fields.choices_label")}
            isLabelHidden
            placeholder={t("custom_fields.choices_label")}
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
            label={t("common.key")}
            isLabelHidden
            placeholder={t("custom_fields.choices_key_placeholder")}
            value={row.key}
            isDisabled={row.isSaved}
            onChange={(key) => update(rows.map((r) => (r.id === row.id ? { ...r, key } : r)))}
            width={180}
          />
          <IconButton
            label={
              row.label
                ? t("custom_fields.choices_remove_named", { label: row.label })
                : t("custom_fields.choices_remove")
            }
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
          label={t("custom_fields.choices_add")}
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
