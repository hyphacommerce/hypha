// Discount codes as the form handles them: compared the way the API stores them, and a
// random one for the "Generate" button, from the same look-alike-free alphabet the API
// generates batches with.

/** No 0/O, 1/I/L: a code read off a flyer is typed right. */
export const CODE_ALPHABET = "ABCDEFGHJKMNPQRSTUVWXYZ23456789";

/** What the store keys a code by: trimmed, Unicode NFKC, uppercase. */
export function normaliseCode(code: string): string {
  return code.trim().normalize("NFKC").toUpperCase();
}

/**
 * A random code of `length` characters from `CODE_ALPHABET`, after `prefix`. `random`
 * fills a byte array (the browser's `crypto.getRandomValues` by default); bytes that
 * would favour some characters are drawn again.
 */
export function generateCode(
  length = 10,
  prefix = "",
  random: (bytes: Uint8Array) => Uint8Array = (bytes) => crypto.getRandomValues(bytes),
): string {
  const size = CODE_ALPHABET.length;
  // The largest multiple of the alphabet's size below 256: bytes from it up are biased.
  const limit = 256 - (256 % size);
  let out = "";
  while (out.length < length) {
    for (const byte of random(new Uint8Array(length))) {
      if (byte >= limit) continue;
      out += CODE_ALPHABET.charAt(byte % size);
      if (out.length === length) break;
    }
  }
  return `${normaliseCode(prefix)}${out}`;
}
