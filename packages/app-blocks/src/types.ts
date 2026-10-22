// The declarative block format, version 1: the TypeScript side of schema/block.v1.json.
// An app's block URL answers a `BlockTree`; any admin renders it with its own components.

export type Gap = 0 | 1 | 2 | 3 | 4;
export type Tone = "neutral" | "info" | "success" | "warning" | "error";
export type BannerTone = Exclude<Tone, "neutral">;

export interface CardElement {
  type: "card";
  title?: string;
  children: BlockElement[];
}

export interface StackElement {
  type: "stack";
  gap?: Gap;
  children: BlockElement[];
}

export interface RowElement {
  type: "row";
  gap?: Gap;
  align?: "start" | "center" | "end" | "between";
  children: BlockElement[];
}

export interface TextElement {
  type: "text";
  value: string;
  variant?: "body" | "supporting" | "code";
  weight?: "normal" | "semibold";
}

export interface HeadingElement {
  type: "heading";
  value: string;
  level?: 2 | 3 | 4;
}

export interface BadgeElement {
  type: "badge";
  value: string;
  tone?: Tone;
}

export interface LinkElement {
  type: "link";
  label: string;
  /** `https://` only. */
  href: string;
  external?: boolean;
}

interface ButtonBase {
  type: "button";
  label: string;
  variant?: "primary" | "secondary" | "destructive";
}
/** Runs an action or navigates within the app's page: one of the two. */
export type ButtonElement =
  | (ButtonBase & { action: string; navigate?: never })
  | (ButtonBase & { navigate: string; action?: never });

export interface DividerElement {
  type: "divider";
}

export interface ImageElement {
  type: "image";
  /** `https://` only. */
  src: string;
  alt: string;
  width?: number;
  height?: number;
}

export interface ListElement {
  type: "list";
  items: { label: string; description?: string }[];
}

export interface TableElement {
  type: "table";
  columns: { key: string; header: string; align?: "start" | "end" }[];
  rows: Record<string, string | number | null>[];
}

export interface KeyValueElement {
  type: "keyvalue";
  items: { label: string; value: string }[];
}

interface FieldBase {
  type: "field";
  name: string;
  label: string;
  description?: string;
  required?: boolean;
}
export type FieldElement =
  | (FieldBase & { input: "text"; value?: string; placeholder?: string })
  | (FieldBase & { input: "number"; value?: number; min?: number; max?: number })
  | (FieldBase & { input: "select"; value?: string; options: { value: string; label: string }[] })
  | (FieldBase & { input: "checkbox"; value?: boolean });

export interface FormElement {
  type: "form";
  action: string;
  submit_label?: string;
  fields: FieldElement[];
}

export interface BannerElement {
  type: "banner";
  tone: BannerTone;
  title: string;
  description?: string;
}

/** Every element version 1 knows. */
export type KnownElement =
  | CardElement
  | StackElement
  | RowElement
  | TextElement
  | HeadingElement
  | BadgeElement
  | LinkElement
  | ButtonElement
  | DividerElement
  | ImageElement
  | ListElement
  | TableElement
  | KeyValueElement
  | FormElement
  | BannerElement;

export type ElementType = KnownElement["type"];

export const ELEMENT_TYPES: readonly ElementType[] = [
  "card",
  "stack",
  "row",
  "text",
  "heading",
  "badge",
  "link",
  "button",
  "divider",
  "image",
  "list",
  "table",
  "keyvalue",
  "form",
  "banner",
];

const KNOWN_TYPES: ReadonlySet<string> = new Set(ELEMENT_TYPES);

/** Whether version 1 knows the element type `type`. */
export function isElementType(type: string): type is ElementType {
  return KNOWN_TYPES.has(type);
}

/**
 * An element of a type a newer version added: allowed in a tree, rendered as nothing.
 * (Typed loosely so trees from newer apps still type-check where they're passed through.)
 */
export interface UnknownElement {
  type: string;
  [key: string]: unknown;
}

export type BlockElement = KnownElement | UnknownElement;

export interface BlockTree {
  version: 1;
  root: BlockElement;
}

/** The values a form submits, by field name. */
export type FormValues = Record<string, string | number | boolean | null>;
