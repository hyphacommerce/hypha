// Server-only: Astryx's own strings (a required field's "Required", a breadcrumb trail's
// label) in the page's language, for its provider (`app/providers.tsx`). Astryx ships a
// catalog per locale; English is built into it.

import { serbianLatin } from "@hyphacommerce/i18n";

import { translated } from "./messages";
import { PSEUDO } from "./pseudo";

/** A catalog as Astryx's provider takes one: its message keys to ICU strings. */
export type AstryxMessages = Readonly<Record<string, string>>;

/**
 * The parts of Astryx's catalog the storefront's components read (a key is
 * `@astryx.<part>.<name>`). Astryx's whole catalog is some 22 KB, most of it for
 * components the shop never renders (tables, calendars, chat), and it's sent with every
 * page; these come to about a tenth. A part missing here shows in English: add it when
 * the shop starts using its component. The pseudo-language shows which are missing.
 */
const PARTS = new Set([
  "banner",
  "breadcrumbs",
  "button",
  "checkboxList",
  "dialog",
  "field",
  "input",
  "link",
  "metadataList",
  "moreMenu",
  "numberInput",
  "popover",
  "selector",
  "spinner",
  "textInput",
  "token",
  "tokenizer",
  "topNav",
  "typeahead",
]);

function parts(catalog: AstryxMessages): AstryxMessages {
  return Object.fromEntries(
    Object.entries(catalog).filter(([key]) => PARTS.has(key.split(".")[1] ?? "")),
  );
}

/**
 * Astryx's catalog for a page written in `language`, loaded when a request asks for it;
 * none for English or a language the storefront has no catalog of its own for. Astryx
 * ships Serbian in Cyrillic only: the two scripts map letter for letter.
 */
export async function astryxMessages(language: string): Promise<AstryxMessages | undefined> {
  if (language === PSEUDO) {
    return parts((await import("@astryxdesign/core/locales/pseudo.generated.js")).default);
  }
  switch (translated(language)) {
    case "sr-Latn": {
      const cyrillic = await import("@astryxdesign/core/locales/sr-SP.generated.js");
      return serbianLatin(parts(cyrillic.default));
    }
    default:
      return undefined;
  }
}
