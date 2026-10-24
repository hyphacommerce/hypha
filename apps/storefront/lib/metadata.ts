// Server-only: a page's metadata for search engines and link previews.

import type { Metadata } from "next";

import { loadCountries, loadStore } from "./api";
import { countryAlternates } from "./seo";
import { link, mode, siteUrl } from "./site";

type Image = { url: string; width?: number | null; height?: number | null; alt?: string | null };

/**
 * A shop page's metadata: its title (the layout's template adds the store's name),
 * description, canonical URL with every country's copy as alternates, and the Open
 * Graph and Twitter previews with `image`. `path` is the page's path inside a country
 * (`/p/mug`).
 */
export async function pageMetadata(input: {
  country: string;
  path: string;
  title?: string;
  description?: string;
  image?: Image | null;
  type?: "website" | "article";
}): Promise<Metadata> {
  const [store, countries] = await Promise.all([loadStore(), loadCountries()]);
  const description = input.description ?? store.description ?? undefined;
  const title = input.title ? `${input.title} · ${store.name}` : store.name;
  const url = link(input.country, input.path);
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
    ...(siteUrl()
      ? {
          alternates: {
            canonical: url,
            languages:
              mode() === "folders"
                ? countryAlternates(
                    store.default_language,
                    countries.map((c) => c.country),
                    (c) => link(c, input.path),
                  )
                : undefined,
          },
        }
      : {}),
    openGraph: {
      type: input.type ?? "website",
      siteName: store.name,
      title,
      description,
      ...(siteUrl() ? { url } : {}),
      locale: `${store.default_language}_${input.country}`,
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
