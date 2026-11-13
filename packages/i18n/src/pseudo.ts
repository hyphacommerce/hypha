// A pseudo-language for development: every letter accented and vowels doubled, so text
// that didn't come from a catalog stands out (it's still plain) and layouts meet strings
// about a third longer, as translations often are. ICU syntax, placeholders and tags
// pass through, so a pseudo catalog formats with the same values.

import type { Messages } from "./translator.ts";

const PLAIN = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
const ACCENTED = "áƀçðéƒĝĥíĵķļɱñöþǫŕšţüṽŵẋýžÁƁÇÐÉƑĜĤÍĴĶĻṀÑÖÞǪŔŠŢÜṼŴẊÝŽ";
const VOWELS = "aeiouAEIOU";
// What an apostrophe quotes in ICU MessageFormat.
const QUOTABLE = "{}<>#|";

/** The catalog with every message in the pseudo-language. */
export function pseudo<M extends Messages>(messages: M): M {
  const out: Record<string, string> = {};
  for (const [key, message] of Object.entries(messages)) out[key] = pseudoMessage(message);
  // oxlint-disable-next-line typescript/no-unsafe-type-assertion -- the same keys, each still a string
  return out as M;
}

/** One ICU message in the pseudo-language. */
export function pseudoMessage(message: string): string {
  let out = "";
  // Braces open an argument (odd depths: names, types, option selectors, styles) and,
  // inside a plural or select, an option's text (even depths), which is text again.
  let depth = 0;
  let i = 0;
  while (i < message.length) {
    const char = message.charAt(i);
    let end = i + 1;
    if (char === "'") {
      end = quotedEnd(message, i);
      out += message.slice(i, end);
    } else if (char === "{") {
      depth += 1;
      out += char;
    } else if (char === "}") {
      depth = Math.max(0, depth - 1);
      out += char;
    } else if (depth % 2 === 1) {
      out += char;
    } else if (char === "<") {
      end = tagEnd(message, i) ?? end;
      out += message.slice(i, end);
    } else {
      out += accent(char);
    }
    i = end;
  }
  return out;
}

function accent(char: string): string {
  const at = PLAIN.indexOf(char);
  if (at === -1) return char;
  const accented = ACCENTED.charAt(at);
  return VOWELS.includes(char) ? accented + accented.toLowerCase() : accented;
}

/**
 * Where the apostrophe at `start` stops mattering: after a doubled one (a literal
 * apostrophe), after the closing one of a quoted run (`'{'`), or after itself when it
 * quotes nothing.
 */
function quotedEnd(message: string, start: number): number {
  const next = message.charAt(start + 1);
  if (next === "'") return start + 2;
  if (next === "" || !QUOTABLE.includes(next)) return start + 1;
  let i = start + 1;
  while (i < message.length) {
    if (message.charAt(i) !== "'") i += 1;
    else if (message.charAt(i + 1) === "'") i += 2;
    else return i + 1;
  }
  return message.length;
}

/** The end of the tag (`<link>`, `</link>`, `<br/>`) opening at `start`, if it is one. */
function tagEnd(message: string, start: number): number | undefined {
  const match = /^<\/?[A-Za-z][A-Za-z0-9_.:-]*\s*\/?>/.exec(message.slice(start));
  return match ? start + match[0].length : undefined;
}
