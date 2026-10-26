// hyphacommerce.com: the marketing pages, the blog and the docs, all built to static
// files. The docs are Starlight under /docs; the API references (/docs/api/*) are Scalar,
// fed the repository's own OpenAPI documents at build time.

import { copyFile, mkdir, readFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import starlight from "@astrojs/starlight";
import { defineConfig } from "astro/config";
import starlightLlmsTxt from "starlight-llms-txt";

// Scalar's browser bundle, served untouched at one URL in dev and in the build. Imported
// through Vite instead, dev rewrites it into a module, which it isn't, and the API pages
// stay blank.
const SCALAR_PATH = "/scalar/standalone.js";
// By path: the file isn't in the package's export map.
const scalarFile = fileURLToPath(
  new URL("./node_modules/@scalar/api-reference/dist/browser/standalone.js", import.meta.url),
);
// The repository's OpenAPI documents, also published as they are at /openapi/*.json:
// stable URLs for agents and code generators (llms.txt links them).
const openapi = (name) => fileURLToPath(new URL(`../../openapi/${name}.json`, import.meta.url));
const SPECS = ["admin", "storefront", "extensions"];

const files = [
  { path: SCALAR_PATH, file: scalarFile, type: "text/javascript" },
  ...SPECS.map((name) => ({
    path: `/openapi/${name}.json`,
    file: openapi(name),
    type: "application/json",
  })),
];

const passthrough = {
  name: "passthrough-files",
  hooks: {
    "astro:server:setup": ({ server }) => {
      for (const f of files) {
        server.middlewares.use(f.path, async (_req, res) => {
          res.setHeader("Content-Type", f.type);
          res.end(await readFile(f.file));
        });
      }
    },
    "astro:build:done": async ({ dir }) => {
      for (const f of files) {
        const out = join(fileURLToPath(dir), f.path);
        await mkdir(dirname(out), { recursive: true });
        await copyFile(f.file, out);
      }
    },
  },
};

export default defineConfig({
  site: "https://hyphacommerce.com",
  output: "static",
  trailingSlash: "ignore",
  integrations: [
    passthrough,
    starlight({
      plugins: [
        starlightLlmsTxt({
          projectName: "Hypha",
          description:
            "Hypha is open-source commerce software: a commerce API in Rust (products, markets, carts, checkout, orders, payments, customers), an admin panel and a storefront. Merchants host it themselves on a server or on Cloudflare Workers, extend it with apps (any language, over HTTP) and spores (Rust plugins), and let AI agents operate the admin over MCP.",
          details: [
            "- Licensed MIT or Apache-2.0. Public preview: APIs may change between 0.x releases.",
            "- Two HTTP surfaces: `/admin/*` (staff sessions, secret keys, apps, agents) and `/storefront/*` (publishable keys). Each store runs its own API; there is no hosted Hypha API.",
            "- Every operation is a command with typed input, required scopes and typed errors with stable `code`s.",
          ].join("\n"),
          optionalLinks: [
            {
              label: "Admin API (OpenAPI 3.1)",
              url: "https://hyphacommerce.com/openapi/admin.json",
            },
            {
              label: "Storefront API (OpenAPI 3.1)",
              url: "https://hyphacommerce.com/openapi/storefront.json",
            },
            {
              label: "Extension points (OpenAPI 3.1 webhooks)",
              url: "https://hyphacommerce.com/openapi/extensions.json",
              description: "What Hypha sends an app mid-flow and the answer it expects.",
            },
            { label: "Source code", url: "https://github.com/hyphacommerce/hypha" },
          ],
        }),
      ],
      title: "Hypha",
      description:
        "Open-source commerce API in Rust, with an admin panel and a storefront. Docs for running, extending and building on Hypha.",
      logo: {
        light: "../admin/public/brand/hypha-wordmark.svg",
        dark: "../admin/public/brand/hypha-wordmark-on-dark.svg",
        replacesTitle: true,
      },
      favicon: "/favicon.svg",
      // Starlight writes the title, description and canonical tags; these add the social
      // card and who publishes the docs (the marketing pages carry the same, from
      // src/lib/seo.ts).
      head: [
        {
          tag: "meta",
          attrs: { property: "og:image", content: "https://hyphacommerce.com/og.png" },
        },
        { tag: "meta", attrs: { property: "og:image:width", content: "1200" } },
        { tag: "meta", attrs: { property: "og:image:height", content: "630" } },
        {
          tag: "meta",
          attrs: { name: "twitter:image", content: "https://hyphacommerce.com/og.png" },
        },
        {
          tag: "script",
          attrs: { type: "application/ld+json" },
          content: JSON.stringify({
            "@context": "https://schema.org",
            "@graph": [
              {
                "@type": "Organization",
                "@id": "https://hyphacommerce.com/#organization",
                name: "Hypha",
                url: "https://hyphacommerce.com/",
                logo: "https://hyphacommerce.com/apple-touch-icon.png",
                sameAs: ["https://github.com/hyphacommerce/hypha"],
              },
              {
                "@type": "WebSite",
                "@id": "https://hyphacommerce.com/#website",
                name: "Hypha",
                url: "https://hyphacommerce.com/",
                publisher: { "@id": "https://hyphacommerce.com/#organization" },
              },
            ],
          }),
        },
      ],
      social: [{ icon: "github", label: "GitHub", href: "https://github.com/hyphacommerce/hypha" }],
      editLink: {
        baseUrl: "https://github.com/hyphacommerce/hypha/edit/main/apps/site/",
      },
      customCss: [
        "@fontsource-variable/geist",
        "@fontsource-variable/geist-mono",
        "./src/styles/fonts.css",
        "./src/styles/starlight.css",
      ],
      sidebar: [
        {
          label: "Start here",
          items: [
            { label: "Introduction", slug: "docs" },
            { label: "Run it locally", slug: "docs/getting-started" },
            { label: "Deploy it", slug: "docs/deploy" },
            { label: "Releases and upgrades", slug: "docs/releases" },
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
