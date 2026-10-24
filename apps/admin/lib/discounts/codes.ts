// Discount codes as the form handles them: compared the way the API stores them, and a
// random one for the "Generate" button, from the same look-alike-free alphabet the API
// generates batches with.

/** No 0/O, 1/I/L: a code read off a flyer is typed right. */
export const CODE_ALPHABET = "ABCDEFGHJKMNPQRSTUVWXYZ23456789";

/** Generated codes' length, the prefix included, as the API takes it. */
export const MIN_GENERATED_LENGTH = 8;
export const MAX_GENERATED_LENGTH = 24;
/** Random characters a generated code needs after its prefix: a guessable code is a
 * discount for anyone. */
export const MIN_RANDOM_CHARACTERS = 8;

/** What the store keys a code by: trimmed, Unicode NFKC, uppercase, then NFKC again
 * (uppercasing can make text that composes differently), as the API does. */
export function normaliseCode(code: string): string {
  return code.trim().normalize("NFKC").toUpperCase().normalize("NFKC");
}

/** Characters as the API counts them: code points, not UTF-16 units. */
// oxlint-disable-next-line typescript/no-misused-spread -- code points are what the API counts
const characters = (text: string) => [...text].length;

/**
 * Why codes of `length` characters can't be generated after `prefix`, or null when they
 * can: the length counts the prefix, and leaves at least `MIN_RANDOM_CHARACTERS` random.
 */
export function generationProblem(length: number, prefix: string): string | null {
  if (!Number.isInteger(length) || length < MIN_GENERATED_LENGTH || length > MAX_GENERATED_LENGTH) {
    return `Length: ${MIN_GENERATED_LENGTH} to ${MAX_GENERATED_LENGTH} characters, the prefix included.`;
  }
  const taken = characters(normaliseCode(prefix));
  if (length - taken >= MIN_RANDOM_CHARACTERS) return null;
  const longest = MAX_GENERATED_LENGTH - MIN_RANDOM_CHARACTERS;
  if (taken > longest) {
    return `The prefix is ${taken} characters: at most ${longest}, so codes keep ${MIN_RANDOM_CHARACTERS} random characters.`;
  }
  return `Codes need ${MIN_RANDOM_CHARACTERS} random characters after the ${taken}-character prefix: a length of ${taken + MIN_RANDOM_CHARACTERS} or more.`;
}

/**
 * The codes in what was typed into "Add codes" (one per line, or separated by commas,
 * semicolons or spaces), each once as the store compares them, in the order typed.
 */
export function codesFrom(text: string): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const raw of text.split(/[\s,;]+/)) {
    const code = raw.trim();
    const key = normaliseCode(code);
    if (code === "" || seen.has(key)) continue;
    seen.add(key);
    out.push(code);
  }
  return out;
}

/**
 * A random code of `length` characters, the prefix included as the API counts it:
 * `prefix` (normalised), then characters from `CODE_ALPHABET`. `random` fills a byte
 * array (the browser's `crypto.getRandomValues` by default); bytes that would favour some
 * characters are drawn again. Throws a `RangeError` with `generationProblem`'s message
 * when the length and prefix leave too few random characters.
 */
export function generateCode(
  length = 12,
  prefix = "",
  random: (bytes: Uint8Array) => Uint8Array = (bytes) => crypto.getRandomValues(bytes),
): string {
  const problem = generationProblem(length, prefix);
  if (problem) throw new RangeError(problem);
  const start = normaliseCode(prefix);
  const wanted = length - characters(start);
  const size = CODE_ALPHABET.length;
  // The largest multiple of the alphabet's size below 256: bytes from it up are biased.
  const limit = 256 - (256 % size);
  let out = "";
  while (out.length < wanted) {
    for (const byte of random(new Uint8Array(wanted))) {
      if (byte >= limit) continue;
      out += CODE_ALPHABET.charAt(byte % size);
      if (out.length === wanted) break;
    }
  }
  return `${start}${out}`;
}
