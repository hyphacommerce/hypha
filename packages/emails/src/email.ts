// One default template: what `build.ts` writes as `<key>.subject.j2`, `<key>.html.j2`
// (the layout's `preheader` and `content` blocks) and `<key>.txt.j2`, once per catalog
// language for a translated email.

import type { ReactNode } from "react";
import type { T } from "./i18n.ts";

export interface Email {
  /** The template's key: the files' names. */
  key: string;
  /** Jinja, one line; not autoescaped on the API's side. */
  subject: string;
  /** Jinja, one line: the text clients show after the subject. */
  preheader: string;
  /** The layout's `content` block, in React Email with `jinja.tsx`'s markers. */
  Content: () => ReactNode;
  /** Jinja: the plain-text body (it doesn't extend the layout). */
  text: string;
}

/** A default in the language `t` speaks. */
export interface Template {
  key: string;
  /**
   * Whether it's written in every catalog's language: the buyer's emails are. Staff and
   * supplier emails are in the store's language, and their defaults are English for now.
   */
  translated: boolean;
  email: (t: T) => Email;
}

/** `{{ path }}`: a variable in a subject, a preheader or a plain-text body. */
export const j = (path: string): string => `{{ ${path} }}`;
