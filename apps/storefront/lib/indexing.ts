// Whether search engines may index this storefront. A demo or staging store sets
// `STOREFRONT_NOINDEX=1` (`hypha.json`'s `noindex`): its pages say `noindex`, robots.txt
// lets crawlers in to read that, and there's no sitemap.

/** Whether this deployment's pages may be indexed (`STOREFRONT_NOINDEX` unset, `0` or
 * `false`). */
export function indexable(value = process.env.STOREFRONT_NOINDEX): boolean {
  const v = value?.trim().toLowerCase();
  return !v || v === "0" || v === "false";
}

/** The `X-Robots-Tag` a page answers with when it may not be indexed. */
export const NOINDEX = "noindex, nofollow";

/**
 * robots.txt for a store kept out of search: everything allowed. A disallowed URL can
 * still be indexed from links to it, without its content; an allowed one is fetched and
 * its `noindex` read, which drops it.
 */
export const NOINDEX_ROBOTS = "User-agent: *\nAllow: /\n";
