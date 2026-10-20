# syntax=docker/dockerfile:1
# The admin or the storefront as a standalone Node server, for a VPS (design note 0025):
#   docker build -f docker/app.Dockerfile --build-arg APP=storefront -t hypha-storefront .
# vinext builds it without the Cloudflare plugin under HYPHA_TARGET=node; it refuses to
# while the app's `cloudflare.config.ts` is there, so the build's copy goes without it.
# Every setting is read at run time from the environment (see the app's .env.example).

ARG NODE_IMAGE=node:24-trixie-slim

FROM ${NODE_IMAGE} AS build
ARG APP
WORKDIR /repo
RUN corepack enable
# The manifests first, so dependencies are installed again only when they change.
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
COPY apps/admin/package.json apps/admin/
COPY apps/storefront/package.json apps/storefront/
# The workspace packages the apps use (sdk, app-blocks, app-bridge-host,
# storefront-elements, …): TypeScript source, all of them, so a new one isn't missed.
COPY packages packages
RUN --mount=type=cache,id=pnpm-store,target=/root/.local/share/pnpm/store \
    pnpm install --frozen-lockfile --filter "${APP}..."
COPY apps/${APP} apps/${APP}
RUN cd apps/${APP} \
    && rm -f cloudflare.config.ts \
    && HYPHA_TARGET=node pnpm exec vite build

FROM ${NODE_IMAGE}
ARG APP
ENV NODE_ENV=production PORT=3000 HOST=0.0.0.0
WORKDIR /app
COPY --from=build --chown=node:node /repo/apps/${APP}/dist/standalone ./
USER node
EXPOSE 3000
CMD ["node", "server.js"]
