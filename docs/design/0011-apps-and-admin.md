# 0011: The TypeScript apps, and the admin

- **Status:** Accepted and built (2026-10-15, branch `feat/admin-app`). Danilo chose the direction in conversation: one
  monorepo, Next.js for the apps (run with vinext so they deploy to Cloudflare like the
  API), and Astryx as the design system. Versions are pinned as of this date.
- **Scope:** how the repository holds the TypeScript apps next to the Rust crates, the
  framework and design system, how the admin authenticates against the API, the typed
  client, and the first admin screens.
- **Not in scope:** the storefront and docs apps (they follow this layout when they
  start), spores (their own note), client customisation beyond theming, and list
  endpoints (0012).
- **Related:** 0008 (sessions, 2FA, API keys), 0009 (rate limits on login), 0010 (the
  admin API, the OpenAPI documents), 0002 (TS-first external apps, spores).

## Decisions

| | Decision |
| --- | --- |
| P1 | **One repository.** The Cargo workspace stays as it is (`crates/*`, later `spores/*`); a pnpm workspace sits beside it (`apps/*`, `packages/*`). The OpenAPI documents in `openapi/` are the contract between the two |
| P2 | **Next.js (App Router) for the apps, built and served with vinext** (Cloudflare's Next.js API on Vite) so they deploy to Workers next to the API. *Rule:* standard Next.js APIs only, nothing vinext-specific, so the same app also runs on Next.js itself (Node, Vercel) if vinext ever falls short |
| P3 | **Astryx** (Meta's design system, StyleX underneath) with its pre-built CSS and a theme package: no StyleX compiler in the build. Versions pinned exactly while it is 0.x |
| P4 | **The admin is a backend for its own frontend.** The browser talks only to the admin app; the admin's server talks to the Hypha admin API. The staff session token lives in an `httpOnly` cookie, so page scripts never see it |
| P5 | **One typed client, generated** from `openapi/*.json` into `packages/sdk` (`openapi-typescript` types, `openapi-fetch` calls). Generated files are committed and checked for freshness, like the documents they come from |
| P6 | **First screens:** login with the second-factor step, the signed-in home (who am I, my scopes), logout. Catalog pages follow the list endpoints |

## 1. Layout (P1)

```text
hypha/
├── Cargo.toml            Rust workspace: crates/* (and spores/* later)
├── package.json          JS workspace root: scripts only, no app code
├── pnpm-workspace.yaml   apps/*, packages/*
├── crates/               core, domains, storage adapters, hosts
├── openapi/              admin.json, storefront.json (generated from the Rust host)
├── apps/
│   └── admin/            the admin panel (Next.js on vinext, Astryx)
└── packages/
    └── sdk/              @hypha/sdk: types and clients generated from openapi/
```

- A change to the API is one commit across all of it: the route, `openapi/*.json`,
  the SDK types, and the admin screen that uses it.
- `spores/` joins the Cargo workspace with the first spore; a spore that needs admin UI
  can ship a TypeScript package next to its crate.
- The storefront (`apps/storefront`) and docs (`apps/docs`) follow the same shape.

## 2. Framework (P2)

Weighed in conversation (2026-10-14):
- **For Next.js:** the most mature Server Components, caching and pre-rendering (which
  the storefront needs), the largest ecosystem (docs tooling such as Fumadocs renders
  OpenAPI), what Hypha's users most likely know, and one framework for all apps.
- **For React Router:** native on any host including Workers, a smaller and more stable
  API, plain request/response data loading.
- **The gap was hosting:** Next.js on anything but Vercel needs an adapter. vinext
  closes most of it ([vinext.dev](https://vinext.dev/); about 94% of the Next.js 16
  API). It calls itself "under active development": known gaps are full
  `cacheComponents`/Partial Prerendering, build-time image and font optimisation, and
  some newer App Router behaviour.

So: Next.js, run with vinext, standard APIs only. The admin uses none of the known gaps
(no Partial Prerendering, no `next/image` pipeline, no `next/font/google`).

## 3. Design system (P3)

- **Astryx** ([astryx.atmeta.com](https://astryx.atmeta.com/)): 180+ accessible React
  components, themes, dark mode, a CLI (component docs, page templates, `swizzle` to
  eject a component) and an MCP server. Beta (0.7.0 on 2026-10-12), React 19+.
- **Why:** clients will want the admin in their own colours and shape. Themes and tokens
  are a better base for that than copied component source.
- **Setup:** the "Next.js (simplest)" path: three CSS imports (reset, components, theme)
  and a `Theme` provider. No StyleX compiler, so vinext's Vite build needs nothing
  special. Custom StyleX styling (`@astryxdesign/build`'s Vite plugin) waits until it is
  needed and tested under vinext.
- **Risk:** 0.x means breaking changes. Exact versions, upgraded deliberately with
  `astryx upgrade` (codemods).
- **Brand theme** (2026-10-15): the admin uses the Hypha theme, `apps/admin/theme/
  hyphaTheme.ts`, made from the brand book. It `extends` Neutral, so it only sets what
  the brand changes: colours (Foxfire lime `#B8E948` with brand-950 ink, moss-tinted
  greys, emerald success), Geist and Geist Mono, dark mode raising surfaces instead of
  shadows. `astryx theme build` compiles it to one self-contained `hypha.css` and
  `hypha.js` (committed, checked with `--check`). The brand book named some tokens Astryx
  doesn't have (`--color-background-brand`, `--color-border-default`); they are mapped onto
  Astryx's own (`--color-accent`, `--color-border`), since unknown variables do nothing.
  A merchant's theme replaces it the same way.

## 4. How the admin authenticates (P4)

```text
browser ──form post──▶ admin (server action) ──POST /admin/auth/login──▶ Hypha
        ◀─ Set-Cookie: hypha_admin_session (httpOnly) ─┘
browser ──page request──▶ admin (server component) ──Bearer <session>──▶ Hypha
```

- **Login** is a server action: it calls `POST /admin/auth/login`, and on success sets
  the cookie to the session token, expiring with the session (`expires_at`). With a
  second factor owed, the challenge token goes in a short-lived cookie of its own and the
  user lands on the code page, which finishes with `POST /admin/auth/login/mfa`.
- **The cookie:** `httpOnly`, `SameSite=Lax`, `Secure` outside local development,
  `Path=/`. Page scripts can't read it, so a script injected into the admin can't steal
  the session; it can only act through the admin's own pages while the user is there.
- **Every page** reads the cookie on the server and calls the API with it. A 401 means
  the session is gone: the cookie is cleared and the user goes to the login page.
- **Logout** revokes the session (`POST /admin/auth/logout`) and clears the cookie.
- **No CORS:** the browser never calls the Hypha API directly, so the API needs no
  cross-origin setup for the admin.
- **Rate limits** (0009): every login reaches Hypha from the admin's server, so Hypha
  would see one address for all staff. The admin forwards the user's address in
  `X-Forwarded-For`, and a deployment lists the admin's address in
  `HYPHA_TRUSTED_PROXIES`. Locally (one machine) this doesn't matter.
- **Configuration:** `HYPHA_API_URL` (server-side only), e.g. `http://127.0.0.1:3917`.

## 5. The typed client (P5)

- `packages/sdk` (`@hypha/sdk`): `src/generated/admin.ts` and `storefront.ts` from
  `openapi-typescript`, plus `createAdminClient(baseUrl, token?)` and
  `createStorefrontClient(baseUrl, publishableKey)` on `openapi-fetch`. Every path, body,
  response and error is typed from the documents.
- `pnpm sdk:generate` regenerates; `pnpm sdk:check` fails when the committed files
  are stale.
- Errors keep the API's shape (`{code, error, details}`), so screens switch on `code`.

## 6. Commands

```bash
pnpm install
pnpm sdk:generate               # after the OpenAPI documents change
pnpm --filter admin dev         # admin on http://localhost:3001 (needs the API running)
pnpm --filter admin build       # vinext production build
pnpm --filter admin typecheck
```

## 7. As built (2026-10-15)

- **Versions pinned:** Next.js 16.4.1 (types and the `next build` fallback), vinext 1.2.0,
  Vite 8.3.0, React 19.3.1, Astryx 0.7.0 (`core`, `theme-neutral`, `cli`), Heroicons
  2.2.0 (the Astryx templates' icons), `openapi-typescript` 7.13.0 and `openapi-fetch`
  0.17.0, pnpm 12.
- **TypeScript 5.9, not 7:** `openapi-typescript` generates through the TypeScript
  compiler API, which the native TypeScript 7 doesn't offer.
- **pnpm's guards, kept on:** dependency install scripts are blocked except `workerd`'s
  (it fetches the Workers runtime the dev server uses); Astryx's only print a setup hint.
  pnpm also holds back packages published within its waiting period; the pinned versions
  newer than that are listed as deliberate exceptions in `pnpm-workspace.yaml`.
- **No `runWorkerFirst`:** the vinext scaffold sends every request to the Worker first.
  Under the Cloudflare Vite plugin's dev server that included Vite's own requests (its
  client, the CSS), so stylesheets came back as pages. Without it, existing static files
  are served directly and everything else reaches the Worker.
- **`HYPHA_API_URL`** is declared as a Worker secret (`bindings.secret()` in
  `cloudflare.config.ts`): read from `.dev.vars` locally, set as a secret per deployment.
  The newer `cf` config doesn't pick variables up without a declaration.
- **Screens from Astryx templates:** the login card from `login-split`, the frame from
  `shell-nav` (`astryx template <id>`).
- **Checked end to end in a browser:** a wrong password shows the API's message, the
  right one opens the shell, page scripts can't read the session cookie, sign-out revokes
  the session, and with 2FA on, login goes through the code page. Both `vite build` and
  `next build` succeed.

## 8. Hard to reverse

- **P1:** cheap to split later; merging repositories back is not.
- **P2:** the app is plain Next.js, so leaving vinext means changing how it is built and
  hosted, not rewriting it.
- **P3:** components are used directly in screens; replacing Astryx later means
  touching every screen. Accepted: the admin is small now.
