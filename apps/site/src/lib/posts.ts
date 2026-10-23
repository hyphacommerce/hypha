import { type CollectionEntry, getCollection } from "astro:content";

export type Post = CollectionEntry<"blog">;

/** The blog's posts, newest first; drafts only in `astro dev`. */
export async function posts(filter: (post: Post) => boolean = () => true): Promise<Post[]> {
  const all = await getCollection("blog", (p) => (import.meta.env.PROD ? !p.data.draft : true));
  return all.filter(filter).toSorted((a, b) => b.data.date.valueOf() - a.data.date.valueOf());
}

export const formatDate = (date: Date) =>
  date.toLocaleDateString("en-GB", { day: "numeric", month: "long", year: "numeric" });

export const shortDate = (date: Date) =>
  date.toLocaleDateString("en-GB", { day: "numeric", month: "short", year: "numeric" });

/** Minutes to read at about 220 words a minute, counting the Markdown source's words. */
export function readingMinutes(post: Post) {
  const words = (post.body ?? "").replace(/<!--[\s\S]*?-->|```[\s\S]*?```/g, " ").split(/\s+/);
  return Math.max(1, Math.round(words.filter(Boolean).length / 220));
}

export const initials = (name: string) =>
  name
    .split(/\s+/)
    .filter((w) => /^[A-Z]/.test(w))
    .slice(0, 2)
    .map((w) => w[0])
    .join("") || name.slice(0, 1).toUpperCase();
