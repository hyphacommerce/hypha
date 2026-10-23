// @hyphacommerce/app-blocks: the declarative block format, version 1. Framework-free: the types
// and the validator. The React renderer is `@hyphacommerce/app-blocks/react`; the JSON Schema is
// `@hyphacommerce/app-blocks/schema/block.v1.json`.

export type * from "./types.ts";
export { ELEMENT_TYPES, isElementType } from "./types.ts";
export { validateBlock, MAX_DEPTH, type Problem, type ValidationResult } from "./validate.ts";
