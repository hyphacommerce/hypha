// Showing the API's prefixed IDs where there's no name to show instead.

/** A prefixed ID shortened for reading: `prod_…a1b2c3`. */
export function shortId(id: string): string {
  const cut = id.indexOf("_");
  return cut > 0 && id.length > cut + 10 ? `${id.slice(0, cut + 1)}…${id.slice(-6)}` : id;
}
