// A React wrapper for app elements: renders the custom element, loads its module in the
// browser, sets props as properties and forwards the documented events. On the server
// it renders the bare tag, which upgrades once the module loads.

import { createElement, useEffect, useEffectEvent, useState } from "react";

import { ELEMENT_EVENT_TYPES, type ElementEventType, type ElementEvents, on } from "./events.ts";
import { loadElement as sharedLoad, setProps } from "./loader.ts";

export type ElementEvent = {
  [T in ElementEventType]: { type: T; detail: ElementEvents[T] };
}[ElementEventType];

export interface HyphaElementProps {
  /** The custom element's tag, as discovery listed it. */
  tag: string;
  /** Its ES module's URL. */
  module: string;
  /** Set as properties on the element (objects stay objects). */
  props?: Record<string, unknown>;
  /** Every documented event; a load failure arrives as `hypha:error`. */
  onEvent?: (event: ElementEvent) => void;
  className?: string;
  /** Replaces the shared loader (tests, a custom registry). */
  load?: typeof sharedLoad;
  /**
   * The mounted element, and `null` once it's gone: for a storefront calling what the
   * element offers (a payment element's `submit()`, with `submit_mode: "external"`).
   */
  onElement?: (element: HTMLElement | null) => void;
}

export function HyphaElement({
  tag,
  module,
  props,
  onEvent,
  className,
  load = sharedLoad,
  onElement,
}: HyphaElementProps) {
  // The mounted element, from a callback ref: the effects below run when it changes
  // (a new tag mounts a new element).
  const [element, setElement] = useState<HTMLElement | null>(null);
  // The latest handler and props, read inside effects without re-running them.
  const emitEvent = useEffectEvent((event: ElementEvent) => onEvent?.(event));
  const applyProps = useEffectEvent((target: HTMLElement) => setProps(target, props ?? {}));
  const reportElement = useEffectEvent((target: HTMLElement | null) => onElement?.(target));

  useEffect(() => {
    reportElement(element);
    return () => reportElement(null);
  }, [element]);

  useEffect(() => {
    if (!element) return;
    const offs = ELEMENT_EVENT_TYPES.map((type) =>
      // oxlint-disable-next-line typescript/no-unsafe-type-assertion -- `on` pairs each type with its own detail; TypeScript can't correlate the two across the union
      on(element, type, (detail) => emitEvent({ type, detail } as ElementEvent)),
    );
    return () => {
      for (const off of offs) off();
    };
  }, [element]);

  useEffect(() => {
    if (!element) return;
    let current = true;
    const run = async () => {
      try {
        await load({ module, tag });
        // Again after the upgrade, in case the element's class replaced what was set before.
        if (current) applyProps(element);
      } catch (error) {
        if (!current) return;
        const message = error instanceof Error ? error.message : String(error);
        emitEvent({ type: "hypha:error", detail: { message } });
      }
    };
    void run();
    return () => {
      current = false;
    };
  }, [element, load, module, tag]);

  useEffect(() => {
    if (element) setProps(element, props ?? {});
  }, [element, props]);

  return createElement(tag, { ref: setElement, class: className });
}
