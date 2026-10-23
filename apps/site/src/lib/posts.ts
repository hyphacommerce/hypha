import { type CollectionEntry, getCollection } from "astro:content";

export type Post = CollectionEntry<"blog">;

/** The blog's posts, newest first; drafts only in `astro dev`. */
export async function posts(filter: (post: Post) => boolean = () => true): Promise<Post[]> {
  const all = await getCollection("blog", (p) => (import.meta.env.PROD ? !p.data.draft : true));
  return all.filter(filter).toSorted((a, b) => b.data.date.valueOf() - a.data.date.valueOf());
}

export const formatDate = (date: Date) =>
  date.toLocaleDateString("en-GB", { day: "numeric", month: "long", year: "numeric" });
