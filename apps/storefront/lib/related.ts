// "You may also like" without a recommendation engine: products alike by what the
// catalogue already says, through the searches a listing makes.

/** What ranking reads of a search hit. */
type Hit = {
  product: string;
  product_type?: string | null;
  vendor?: string | null;
  available?: boolean | null;
};

/** The product the shelf is for. */
type Subject = { id: string; product_type?: string | null; vendor?: string | null };

/**
 * The searches' hits ranked for `subject`, best first: sharing a tag counts 2, the same
 * type 2, the same maker 1, and what can be bought now goes before what can't; ties keep
 * the order the searches gave (newest first). The newest products fill a shelf the
 * others leave short. Each product once, never the subject itself.
 */
export function rankRelated<H extends Hit>(
  subject: Subject,
  found: { tags: H[]; type: H[]; vendor: H[]; newest: H[] },
  limit: number,
): H[] {
  const tagged = new Set(found.tags.map((h) => h.product));
  const seen = new Map<string, { hit: H; score: number; order: number }>();
  for (const hit of [...found.tags, ...found.type, ...found.vendor, ...found.newest]) {
    if (hit.product === subject.id || seen.has(hit.product)) continue;
    const score =
      (tagged.has(hit.product) ? 2 : 0) +
      (subject.product_type && hit.product_type === subject.product_type ? 2 : 0) +
      (subject.vendor && hit.vendor === subject.vendor ? 1 : 0);
    seen.set(hit.product, { hit, score, order: seen.size });
  }
  return [...seen.values()]
    .toSorted(
      (a, b) =>
        Number(b.hit.available !== false) - Number(a.hit.available !== false) ||
        b.score - a.score ||
        a.order - b.order,
    )
    .slice(0, limit)
    .map((r) => r.hit);
}
