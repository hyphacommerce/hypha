import rss from "@astrojs/rss";
import type { APIContext } from "astro";

import { SITE_DESCRIPTION, SITE_TITLE } from "../consts";
import { posts } from "../lib/posts";

export async function GET(context: APIContext) {
  const items = await posts();
  return rss({
    title: `${SITE_TITLE} blog`,
    description: SITE_DESCRIPTION,
    site: context.site ?? "https://hyphacommerce.com",
    items: items.map((post) => ({
      title: post.data.title,
      description: post.data.description,
      pubDate: post.data.date,
      link: `/blog/${post.id}/`,
      categories: post.data.tags,
    })),
  });
}
