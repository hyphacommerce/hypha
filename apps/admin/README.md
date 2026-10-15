# Hypha admin

The admin panel: Next.js (App Router) run with [vinext](https://vinext.dev/) on Vite,
[Astryx](https://astryx.atmeta.com/) components, the Hypha admin API behind it. Design
note: [`docs/design/0011-apps-and-admin.md`](../../docs/design/0011-apps-and-admin.md).

## Run it locally

```bash
# 1. Postgres and the S3 server, then the API (from the repo root) with an owner:
docker compose up -d
# HYPHA_SECRET_KEY: generate once (`openssl rand -hex 32`) and keep it; it seals 2FA secrets.
HYPHA_BIND=127.0.0.1:3917 HYPHA_SECRET_KEY=<your key> \
  HYPHA_OWNER_EMAIL=owner@example.com HYPHA_OWNER_PASSWORD=change-me-please \
  cargo run -p hypha-host-axum

# 2. The admin's settings (once):
cp apps/admin/.env.example apps/admin/.dev.vars

# 3. The admin, on http://localhost:3001:
pnpm install
pnpm admin:dev
```

## How it's put together

- **The browser never talks to the API.** Server actions and server components call it
  (`lib/hypha.ts`, through `@hypha/sdk`); the staff session lives in an `httpOnly`
  cookie (`hypha_admin_session`) page scripts can't read.
- **Login** (`app/login`): email and password, then a code page when 2FA is on.
- **Signed-in pages** live in `app/(admin)`; its layout checks the session on every
  request (`requireStaff`). Pages are server components that read with
  `loaded(await client.GET(…))` (a dead session goes to login, a missing resource to
  404); changes go through server actions next to them (`actions.ts`), which return
  `{error}` with the message from `describe` and revalidate the pages they affect.
- **Built so far:** products (list with status filter, detail with status changes,
  add), orders (list, detail, mark as paid), media (library with uploads, detail with
  alt text and title, delete), Settings › Store (rename).
- **Uploads** go from the browser straight to storage: a server action asks Hypha for a
  signed upload (`media/actions.ts`), the browser `PUT`s the file to it, and another
  action has Hypha check the stored file. The file never passes through the admin, and
  the session token stays server-side (design note 0013). Lists page
  forward with the API's opaque cursors (`components/pager.tsx`, design note 0012).
- **Money and dates** go through `lib/format.ts`: integer minor units, a fixed locale
  and UTC, so server and browser render the same text.
- **Screens** start from Astryx templates (`pnpm exec astryx template --list`): the
  login card from `login-split`, the frame from `shell-nav`. `pnpm exec astryx component
  <Name>` prints any component's props and guidance.
- **Types** come from the API's OpenAPI documents: after an API change, `cargo run -p
  hypha-host-axum -- openapi` and then `pnpm sdk:generate`.

## Builds

- `pnpm build`: the vinext build (what deploys to Cloudflare Workers).
- `pnpm exec next build`: plain Next.js, kept working so the app isn't tied to vinext.
