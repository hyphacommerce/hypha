// Caching the API's answers in the admin (design note 0024, section 6). Reads repeat a
// lot (going back to a list, flipping a filter, the layout and the page reading the
// same store), so a staff session's GETs are cached briefly, keyed by the session and
// the URL. Every write the session makes expires its whole cache at once, so nobody
// sees their own change missing. What others change (another staff member, a storefront
// order) shows within the read's lifetime: a few seconds for live lists, half a minute
// for the rest.
//
// Server only: the session token never leaves the admin's server, and the cache key
// holds its hash, not the token.

import { revalidateTag, unstable_cache, updateTag } from "next/cache";

/** Seconds a read stays fresh, by path: live lists briefly, never what must be current. */
function lifetime(path: string): number | null {
  if (/\/(status|me)$/.test(path) || path.includes("/auth/")) return null;
  if (/^\/admin\/(orders|fulfillments|apps\/[^/]+\/deliveries)/.test(path)) return 5;
  if (/^\/admin\/(currencies|countries)$/.test(path)) return 3600;
  return 30;
}

/** A cached answer: what a Response needs to be rebuilt. */
type Stored = { status: number; body: string; contentType: string | null };

/** Thrown inside the cache for an answer it mustn't keep (an error), with the answer. */
class Uncached extends Error {
  constructor(readonly stored: Stored) {
    super("uncached answer");
  }
}

async function sha256(text: string): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text));
  return Array.from(new Uint8Array(digest), (b) => b.toString(16).padStart(2, "0")).join("");
}

function rebuild(stored: Stored): Response {
  return new Response(stored.body, {
    status: stored.status,
    headers: stored.contentType ? { "content-type": stored.contentType } : {},
  });
}

/** Expires every read the session has cached; in a server action, at once. */
function expire(tag: string) {
  try {
    updateTag(tag);
  } catch {
    // Outside a server action (a route handler): expire on the next read.
    revalidateTag(tag, { expire: 0 });
  }
}

/**
 * A `fetch` for the session holding `token`: GETs come from its cache while fresh (only
 * successful answers are kept), and any successful write expires that cache.
 */
export function sessionFetch(token: string): typeof fetch {
  const tagged = sha256(token).then((hash) => `admin-session:${hash}`);
  return async (input, init) => {
    const request = new Request(input, init);
    const tag = await tagged;
    if (request.method !== "GET") {
      const response = await fetch(request);
      if (response.ok) expire(tag);
      return response;
    }
    const url = new URL(request.url);
    const seconds = lifetime(url.pathname);
    if (seconds === null) return fetch(request);
    const headers = Object.fromEntries(request.headers);
    const read = unstable_cache(
      async (): Promise<Stored> => {
        const response = await fetch(url, { headers });
        const stored = {
          status: response.status,
          body: await response.text(),
          contentType: response.headers.get("content-type"),
        };
        if (!response.ok) throw new Uncached(stored);
        return stored;
      },
      [tag, url.pathname + url.search],
      { tags: [tag], revalidate: seconds },
    );
    try {
      return rebuild(await read());
    } catch (e) {
      if (e instanceof Uncached) return rebuild(e.stored);
      throw e;
    }
  };
}
