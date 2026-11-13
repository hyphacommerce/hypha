// Server-only: a page's metadata for search engines and link previews.

import { localeOf } from "@hyphacommerce/i18n";
import type { Metadata } from "next";

import { loadCountries, loadStore, pageLocale } from "./api";
import { alternatePlaces } from "./routing";
import { localeAlternates } from "./seo";
import { link, mode, siteUrl } from "./site";

type Image = { url: string; width?: number | null; height?: number | null; alt?: string | null };

/**
 * A page's alternates by `hreflang`: its copy in every country and language
 * (`alternatePlaces`), `path` being its path inside a place (`/p/mug`). `among` limits
 * them to some countries (a product only where it's sold).
 */
export async function alternatesOf(
  path: string,
  among?: readonly string[],
): Promise<Record<string, string> | undefined> {
  const countries = (await loadCountries()).filter((c) => !among || among.includes(c.country));
  return localeAlternates(
    alternatePlaces(mode(), countries).map((p) => ({ tag: p.tag, href: link(p.segment, path) })),
  );
}

/**
 * A shop page's metadata: its title (the layout's template adds the store's name),
 * description, its own URL as canonical with its copy in every country and language as
 * alternates, and the Open Graph and Twitter previews with `image`. `segment` is the
 * page's locale segment (`params.country`), `path` its path inside it (`/p/mug`).
 */
export async function pageMetadata(input: {
  segment: string;
  path: string;
  title?: string;
  description?: string;
  image?: Image | null;
  type?: "website" | "article";
}): Promise<Metadata> {
  const place = await pageLocale(input.segment);
  const [store, languages] = await Promise.all([
    loadStore(place.country, place.language),
    siteUrl() ? alternatesOf(input.path) : undefined,
  ]);
  const description = input.description ?? store.description ?? undefined;
  const title = input.title ? `${input.title} · ${store.name}` : store.name;
  const url = link(place.segment, input.path);
  const images = input.image
    ? [
        {
          url: input.image.url,
          ...(input.image.width ? { width: input.image.width } : {}),
          ...(input.image.height ? { height: input.image.height } : {}),
          ...(input.image.alt ? { alt: input.image.alt } : {}),
        },
      ]
    : undefined;
  return {
    ...(input.title ? { title: input.title } : {}),
    description,
    // Relative links need the origin to resolve against; without it, none.
    ...(siteUrl() ? { alternates: { canonical: url, languages } } : {}),
    openGraph: {
      type: input.type ?? "website",
      siteName: store.name,
      title,
      description,
      ...(siteUrl() ? { url } : {}),
      // Open Graph spells a locale `de_CH`.
      locale: localeOf(place.language, place.country).replace(/-/g, "_"),
      images,
    },
    twitter: {
      card: images ? "summary_large_image" : "summary",
      title,
      description,
      images: images?.map((i) => i.url),
    },
  };
}
