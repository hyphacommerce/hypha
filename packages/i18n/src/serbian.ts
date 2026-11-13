// Serbian Cyrillic to Serbian Latin: the two scripts map letter for letter, so a catalog
// shipped in one (a component library's) can be shown in the other.

import type { Messages } from "./translator.ts";

const CYRILLIC = "абвгдђежзијклљмнњопрстћуфхцчџш";
const LATIN = "a b v g d đ e ž z i j k l lj m n nj o p r s t ć u f h c č dž š".split(" ");

/** The catalog with every message's Serbian Cyrillic in Serbian Latin. */
export function serbianLatin<M extends Messages>(messages: M): M {
  const out: Record<string, string> = {};
  for (const [key, message] of Object.entries(messages)) out[key] = toSerbianLatin(message);
  // oxlint-disable-next-line typescript/no-unsafe-type-assertion -- the same keys, each still a string
  return out as M;
}

/**
 * Serbian Cyrillic in `text` as Serbian Latin; everything else (Latin text, ICU syntax,
 * placeholders, tags) as it is. A capital that becomes two letters is `Lj` in a
 * capitalised word and `LJ` in one all in capitals.
 */
export function toSerbianLatin(text: string): string {
  let out = "";
  // By UTF-16 unit: every Serbian Cyrillic letter is one, and anything else is copied.
  for (let i = 0; i < text.length; i += 1) {
    const char = text.charAt(i);
    const lower = char.toLowerCase();
    const at = CYRILLIC.indexOf(lower);
    const latin = at === -1 ? undefined : LATIN[at];
    if (latin === undefined) {
      out += char;
    } else if (char === lower) {
      out += latin;
    } else if (latin.length === 1 || inCapitals(text, i)) {
      out += latin.toUpperCase();
    } else {
      out += latin.charAt(0).toUpperCase() + latin.slice(1);
    }
  }
  return out;
}

/** Whether the capital at `i` stands in a word of capitals: by the next letter, else the one before. */
function inCapitals(text: string, i: number): boolean {
  const next = text.charAt(i + 1);
  if (isLetter(next)) return isUpper(next);
  const before = text.charAt(i - 1);
  return isLetter(before) && isUpper(before);
}

function isLetter(char: string): boolean {
  return /\p{L}/u.test(char);
}

function isUpper(char: string): boolean {
  return char !== char.toLowerCase();
}
