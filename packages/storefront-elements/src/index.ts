// @hypha/storefront-elements: loading app elements into any storefront
// (docs/apps/elements.md). Framework-free; the React wrapper is
// `@hypha/storefront-elements/react`.

export {
  createElementLoader,
  loadElement,
  mountElement,
  setProps,
  ElementLoadError,
  type ElementRef,
  type ElementRegistry,
  type LoaderOptions,
  type MountOptions,
} from "./loader.ts";
export {
  on,
  emit,
  ELEMENT_EVENT_TYPES,
  type ElementEvents,
  type ElementEventType,
} from "./events.ts";
