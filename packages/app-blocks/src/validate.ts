// Checks a block tree against version 1's rules (schema/block.v1.json, which the tests
// hold this to), without a JSON Schema library. Elements of a type version 1 doesn't
// know are warnings, not errors: hosts skip them (forward compatibility).

import { type BlockTree, isElementType } from "./types.ts";

export interface Problem {
  /** Where, e.g. `root.children[1].label`. */
  path: string;
  message: string;
}

export type ValidationResult =
  | { valid: true; tree: BlockTree; errors: []; warnings: Problem[] }
  | { valid: false; errors: Problem[]; warnings: Problem[] };

/** Trees nested deeper than this are refused (not expressible in the schema). */
export const MAX_DEPTH = 16;

type Obj = Record<string, unknown>;

class Checker {
  errors: Problem[] = [];
  warnings: Problem[] = [];

  error(path: string, message: string) {
    this.errors.push({ path, message });
  }

  object(value: unknown, path: string): value is Obj {
    if (typeof value === "object" && value !== null && !Array.isArray(value)) return true;
    this.error(path, "must be an object");
    return false;
  }

  /** Fails on keys outside `allowed`. */
  keys(value: Obj, path: string, allowed: readonly string[]) {
    for (const key of Object.keys(value))
      if (!allowed.includes(key)) this.error(`${path}.${key}`, "is not allowed here");
  }

  string(
    value: Obj,
    key: string,
    path: string,
    rules: { required?: boolean; min?: number; max: number; pattern?: RegExp },
  ) {
    const v = value[key];
    if (v === undefined) {
      if (rules.required) this.error(`${path}.${key}`, "is required");
      return;
    }
    if (typeof v !== "string") return this.error(`${path}.${key}`, "must be a string");
    if (v.length < (rules.min ?? 0))
      this.error(`${path}.${key}`, `must have at least ${rules.min} character(s)`);
    if (v.length > rules.max)
      this.error(`${path}.${key}`, `must have at most ${rules.max} characters`);
    if (rules.pattern && !rules.pattern.test(v))
      this.error(`${path}.${key}`, `must match ${rules.pattern.source}`);
  }

  label(value: Obj, key: string, path: string, required = true) {
    this.string(value, key, path, { required, min: 1, max: 200 });
  }

  text(value: Obj, key: string, path: string, required = false) {
    this.string(value, key, path, { required, max: 2000 });
  }

  https(value: Obj, key: string, path: string) {
    this.string(value, key, path, { required: true, max: 2048, pattern: /^https:\/\// });
  }

  oneOf(value: Obj, key: string, path: string, options: readonly unknown[], required = false) {
    const v = value[key];
    if (v === undefined) {
      if (required) this.error(`${path}.${key}`, "is required");
      return;
    }
    if (!options.includes(v))
      this.error(
        `${path}.${key}`,
        `must be one of ${options.map((o) => JSON.stringify(o)).join(", ")}`,
      );
  }

  boolean(value: Obj, key: string, path: string) {
    if (value[key] !== undefined && typeof value[key] !== "boolean")
      this.error(`${path}.${key}`, "must be a boolean");
  }

  number(value: Obj, key: string, path: string, integer?: { min: number; max: number }) {
    const v = value[key];
    if (v === undefined) return;
    if (typeof v !== "number" || !Number.isFinite(v))
      return this.error(`${path}.${key}`, "must be a number");
    if (integer && (!Number.isInteger(v) || v < integer.min || v > integer.max))
      this.error(`${path}.${key}`, `must be an integer from ${integer.min} to ${integer.max}`);
  }

  array(value: Obj, key: string, path: string, rules: { min?: number; max: number }): unknown[] {
    const v = value[key];
    if (v === undefined) {
      this.error(`${path}.${key}`, "is required");
      return [];
    }
    if (!Array.isArray(v)) {
      this.error(`${path}.${key}`, "must be an array");
      return [];
    }
    if (v.length < (rules.min ?? 0))
      this.error(`${path}.${key}`, `must have at least ${rules.min} item(s)`);
    if (v.length > rules.max) this.error(`${path}.${key}`, `must have at most ${rules.max} items`);
    return v;
  }

  children(value: Obj, path: string, depth: number) {
    this.array(value, "children", path, { max: 100 }).forEach((child, i) =>
      this.element(child, `${path}.children[${i}]`, depth + 1),
    );
  }

  element(value: unknown, path: string, depth: number) {
    if (!this.object(value, path)) return;
    if (depth > MAX_DEPTH) return this.error(path, `is nested deeper than ${MAX_DEPTH}`);
    const type = value.type;
    if (typeof type !== "string" || type.length === 0)
      return this.error(`${path}.type`, "must be a non-empty string");
    if (type === "field") return this.error(path, "a field belongs in a form's fields");
    if (!isElementType(type)) {
      this.warnings.push({
        path,
        message: `unknown element type ${JSON.stringify(type)}: skipped`,
      });
      return;
    }
    const known = type;
    const allow = (...keys: string[]) => this.keys(value, path, ["type", ...keys]);
    switch (known) {
      case "card":
        allow("title", "children");
        this.label(value, "title", path, false);
        return this.children(value, path, depth);
      case "stack":
        allow("gap", "children");
        this.oneOf(value, "gap", path, [0, 1, 2, 3, 4]);
        return this.children(value, path, depth);
      case "row":
        allow("gap", "align", "children");
        this.oneOf(value, "gap", path, [0, 1, 2, 3, 4]);
        this.oneOf(value, "align", path, ["start", "center", "end", "between"]);
        return this.children(value, path, depth);
      case "text":
        allow("value", "variant", "weight");
        this.text(value, "value", path, true);
        this.oneOf(value, "variant", path, ["body", "supporting", "code"]);
        return this.oneOf(value, "weight", path, ["normal", "semibold"]);
      case "heading":
        allow("value", "level");
        this.label(value, "value", path);
        return this.oneOf(value, "level", path, [2, 3, 4]);
      case "badge":
        allow("value", "tone");
        this.string(value, "value", path, { required: true, min: 1, max: 50 });
        return this.oneOf(value, "tone", path, ["neutral", "info", "success", "warning", "error"]);
      case "link":
        allow("label", "href", "external");
        this.label(value, "label", path);
        this.https(value, "href", path);
        return this.boolean(value, "external", path);
      case "button": {
        allow("label", "action", "navigate", "variant");
        this.label(value, "label", path);
        this.string(value, "action", path, { max: 64, pattern: ACTION_ID });
        this.string(value, "navigate", path, { max: 2048, pattern: PATH });
        if ((value.action === undefined) === (value.navigate === undefined))
          this.error(path, "needs exactly one of action and navigate");
        return this.oneOf(value, "variant", path, ["primary", "secondary", "destructive"]);
      }
      case "divider":
        return allow();
      case "image":
        allow("src", "alt", "width", "height");
        this.https(value, "src", path);
        this.string(value, "alt", path, { required: true, max: 300 });
        this.number(value, "width", path, { min: 1, max: 2000 });
        return this.number(value, "height", path, { min: 1, max: 2000 });
      case "list":
        allow("items");
        return this.array(value, "items", path, { max: 100 }).forEach((item, i) => {
          const at = `${path}.items[${i}]`;
          if (!this.object(item, at)) return;
          this.keys(item, at, ["label", "description"]);
          this.label(item, "label", at);
          this.text(item, "description", at);
        });
      case "table": {
        allow("columns", "rows");
        this.array(value, "columns", path, { min: 1, max: 12 }).forEach((column, i) => {
          const at = `${path}.columns[${i}]`;
          if (!this.object(column, at)) return;
          this.keys(column, at, ["key", "header", "align"]);
          this.string(column, "key", at, {
            required: true,
            max: 64,
            pattern: /^[A-Za-z0-9_]{1,64}$/,
          });
          this.label(column, "header", at);
          this.oneOf(column, "align", at, ["start", "end"]);
        });
        return this.array(value, "rows", path, { max: 200 }).forEach((row, i) => {
          const at = `${path}.rows[${i}]`;
          if (!this.object(row, at)) return;
          for (const [key, cell] of Object.entries(row))
            if (cell !== null && typeof cell !== "string" && typeof cell !== "number")
              this.error(`${at}.${key}`, "must be a string, a number or null");
        });
      }
      case "keyvalue":
        allow("items");
        return this.array(value, "items", path, { max: 100 }).forEach((item, i) => {
          const at = `${path}.items[${i}]`;
          if (!this.object(item, at)) return;
          this.keys(item, at, ["label", "value"]);
          this.label(item, "label", at);
          this.text(item, "value", at, true);
        });
      case "form":
        allow("action", "submit_label", "fields");
        this.string(value, "action", path, { required: true, max: 64, pattern: ACTION_ID });
        this.label(value, "submit_label", path, false);
        return this.array(value, "fields", path, { min: 1, max: 50 }).forEach((field, i) =>
          this.field(field, `${path}.fields[${i}]`),
        );
      case "banner":
        allow("tone", "title", "description");
        this.oneOf(value, "tone", path, ["info", "success", "warning", "error"], true);
        this.label(value, "title", path);
        return this.text(value, "description", path);
    }
  }

  field(value: unknown, path: string) {
    if (!this.object(value, path)) return;
    if (value.type !== "field") this.error(`${path}.type`, 'must be "field"');
    this.oneOf(value, "input", path, ["text", "number", "select", "checkbox"], true);
    this.string(value, "name", path, {
      required: true,
      max: 64,
      pattern: /^[A-Za-z][A-Za-z0-9_]{0,63}$/,
    });
    this.label(value, "label", path);
    this.text(value, "description", path);
    this.boolean(value, "required", path);
    const common = ["type", "input", "name", "label", "description", "required", "value"];
    switch (value.input) {
      case "text":
        this.keys(value, path, [...common, "placeholder"]);
        this.text(value, "value", path);
        return this.label(value, "placeholder", path, false);
      case "number":
        this.keys(value, path, [...common, "min", "max"]);
        this.number(value, "value", path);
        this.number(value, "min", path);
        return this.number(value, "max", path);
      case "select":
        this.keys(value, path, [...common, "options"]);
        if (value.value !== undefined && typeof value.value !== "string")
          this.error(`${path}.value`, "must be a string");
        return this.array(value, "options", path, { min: 1, max: 100 }).forEach((option, i) => {
          const at = `${path}.options[${i}]`;
          if (!this.object(option, at)) return;
          this.keys(option, at, ["value", "label"]);
          this.string(option, "value", at, { required: true, max: 200 });
          this.label(option, "label", at);
        });
      case "checkbox":
        this.keys(value, path, common);
        return this.boolean(value, "value", path);
    }
  }
}

const ACTION_ID = /^[a-z][a-z0-9_.-]{0,63}$/;
const PATH = /^\/(?!\/)/;

/** Whether `value` is a valid version-1 tree, with every problem found. */
export function validateBlock(value: unknown): ValidationResult {
  const checker = new Checker();
  if (checker.object(value, "$")) {
    checker.keys(value, "$", ["version", "root"]);
    if (value.version !== 1) checker.error("$.version", "must be 1");
    if (value.root === undefined) checker.error("$.root", "is required");
    else checker.element(value.root, "root", 1);
  }
  const { errors, warnings } = checker;
  if (!passed(value, errors)) return { valid: false, errors, warnings };
  return { valid: true, tree: value, errors: [], warnings };
}

/** A tree the checks found nothing wrong with is a `BlockTree`: that's what they check. */
function passed(_value: unknown, errors: Problem[]): _value is BlockTree {
  return errors.length === 0;
}
