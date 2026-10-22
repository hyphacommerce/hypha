// One default template: what `build.ts` writes as `<key>.subject.j2`, `<key>.html.j2`
// (the layout's `preheader` and `content` blocks) and `<key>.txt.j2`.

import type { ReactNode } from "react";

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
