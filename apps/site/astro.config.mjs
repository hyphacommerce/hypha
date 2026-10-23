// hyphacommerce.com: the marketing pages, the blog and the docs, all built to static
// files. The docs are Starlight under /docs; the API references (/docs/api/*) are Scalar,
// fed the repository's own OpenAPI documents at build time.

import starlight from "@astrojs/starlight";
import { defineConfig } from "astro/config";

export default defineConfig({
  site: "https://hyphacommerce.com",
  output: "static",
  trailingSlash: "ignore",
  integrations: [
    starlight({
      title: "Hypha",
      description:
        "Open-source commerce API in Rust, with an admin panel and a storefront. Docs for running, extending and building on Hypha.",
      logo: {
        light: "../admin/public/brand/hypha-wordmark.svg",
        dark: "../admin/public/brand/hypha-wordmark-on-dark.svg",
        replacesTitle: true,
      },
      favicon: "/favicon.svg",
      social: [{ icon: "github", label: "GitHub", href: "https://github.com/hyphacommerce/hypha" }],
      editLink: {
        baseUrl: "https://github.com/hyphacommerce/hypha/edit/main/apps/site/",
      },
      customCss: [
        "@fontsource-variable/geist",
        "@fontsource-variable/geist-mono",
        "./src/styles/starlight.css",
      ],
      sidebar: [
        {
          label: "Start here",
          items: [
            { label: "Introduction", slug: "docs" },
            { label: "Run it locally", slug: "docs/getting-started" },
            { label: "Deploy it", slug: "docs/deploy" },
          ],
        },
        { label: "Concepts", items: [{ autogenerate: { directory: "docs/concepts" } }] },
        { label: "Guides", items: [{ autogenerate: { directory: "docs/guides" } }] },
        {
          label: "API reference",
          items: [
            { label: "Admin API", link: "/docs/api/admin" },
            { label: "Storefront API", link: "/docs/api/storefront" },
            { label: "Extension points", link: "/docs/api/extensions" },
          ],
        },
      ],
    }),
  ],
});
