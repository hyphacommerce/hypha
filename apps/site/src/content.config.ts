import { docsLoader } from "@astrojs/starlight/loaders";
import { docsSchema } from "@astrojs/starlight/schema";
import { defineCollection } from "astro:content";
import { glob } from "astro/loaders";
import { z } from "astro/zod";

export const collections = {
  docs: defineCollection({ loader: docsLoader(), schema: docsSchema() }),
  // Posts, release announcements among them (`release: true`, with the version they
  // announce). Drafts show in `astro dev` only.
  blog: defineCollection({
    loader: glob({ pattern: "**/*.{md,mdx}", base: "./src/content/blog" }),
    schema: z.object({
      title: z.string(),
      description: z.string(),
      date: z.coerce.date(),
      author: z.string().default("The Hypha team"),
      tags: z.array(z.string()).default([]),
      draft: z.boolean().default(false),
      release: z.boolean().default(false),
      version: z.string().optional(),
    }),
  }),
  // The repository's CHANGELOG.md, written by release-please; empty until the first
  // release.
  changelog: defineCollection({
    loader: glob({ pattern: "CHANGELOG.md", base: "../.." }),
  }),
};
