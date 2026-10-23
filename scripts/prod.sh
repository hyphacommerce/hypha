# !/usr/bin/env bash
# Runs the production builds locally, to see how they behave: the API built with
# `--release`, and the admin and storefront built (`vite build`) and served from that
# build (`vite preview`: the built worker in workerd, as on Cloudflare). Same ports,
# databases, secret key and `.dev.vars` as `pnpm dev` (scripts/dev.sh), so stop that
# first. Ctrl+C stops all three.
#
#   pnpm prod                          # the hypha_preview database
#   HYPHA_DEV_DB=hypha_demo pnpm prod  # the seeded demo store
#   HYPHA_PROD_SKIP_BUILD=1 pnpm prod  # serve the last builds again
#
# Production here means optimised code, static pages served from cache with ISR and
# the revalidation webhook, and no dev overlay; it's still plain HTTP on localhost.
set -euo pipefail
cd "$(dirname "$0")/.."

# First-party spores to build the API with: HYPHA_SPORES=stripe (comma-separated), each
# from its checkout beside this one (`../spore-stripe`) or else cloned at its head, in a
# host crate of their own (`.hypha-build/dev/host`).
API_CRATE=(-p hypha-host-axum)
# `:-`: set -u refuses an unset variable (bash 4+).
if [ -n "${HYPHA_SPORES:-}" ]; then
  API_CRATE=(--manifest-path "$(node packages/cli/src/dev-host.ts "$HYPHA_SPORES")")
  # The workspace's target, so Hypha's crates aren't compiled a second time.
  export CARGO_TARGET_DIR="$PWD/target"
fi

API_PORT=3917

if ! docker info >/dev/null 2>&1; then
  echo "Docker isn't running: start Docker Desktop, then run this again." >&2
  exit 1
fi
docker compose up -d --wait

if [ -z "${DATABASE_URL:-}" ]; then
  DB="${HYPHA_DEV_DB:-hypha_preview}"
  docker compose exec -T postgres psql -U hypha -tAc \
    "SELECT 1 FROM pg_database WHERE datname = '$DB'" | grep -q 1 ||
    docker compose exec -T postgres psql -U hypha -qc "CREATE DATABASE $DB"
  export DATABASE_URL="postgres://hypha:hypha@localhost:5432/$DB"
fi

if [ ! -s .dev-secret-key ]; then
  openssl rand -hex 32 > .dev-secret-key
fi

for app in admin storefront; do
  if [ ! -f "apps/$app/.dev.vars" ]; then
    echo "apps/$app/.dev.vars is missing: copy apps/$app/.env.example and fill it in." >&2
    exit 1
  fi
done

for port in "$API_PORT" 3001 3002; do
  if lsof -iTCP:"$port" -sTCP:LISTEN >/dev/null 2>&1; then
    echo "Port $port is taken (is \`pnpm dev\` still running?)." >&2
    exit 1
  fi
done

# Built before anything starts, so a failing build stops here.
if [ -z "${HYPHA_PROD_SKIP_BUILD:-}" ]; then
  echo "Building the API (release), the admin and the storefront…"
  cargo build --release -q "${API_CRATE[@]}"
  pnpm --filter admin build
  pnpm --filter storefront build
fi

pids=()
kill_tree() {
  local child
  for child in $(pgrep -P "$1"); do kill_tree "$child"; done
  kill "$1" 2>/dev/null || true
}
stop() {
  trap - INT TERM EXIT
  local pid
  for pid in ${pids[@]+"${pids[@]}"}; do kill_tree "$pid"; done
  wait 2>/dev/null || true
}
trap stop INT TERM EXIT

label() { sed -u "s/^/[$1] /"; }


# Through cargo (already built, so it starts at once) for `.cargo/config.toml`'s
# HYPHA_S3_* settings, which point it at the compose file storage.
echo "Starting the API on :${API_PORT}…"
HYPHA_BIND="127.0.0.1:$API_PORT" \
  HYPHA_SECRET_KEY="$(cat .dev-secret-key)" \
  HYPHA_ALLOW_PRIVATE_CALLBACKS=true \
  cargo run --release -q "${API_CRATE[@]}" > >(label api) 2>&1 &
pids+=($!)

until curl -sf "http://127.0.0.1:$API_PORT/health" >/dev/null; do
  if ! kill -0 "${pids[0]}" 2>/dev/null; then
    echo "The API stopped before it was ready (see [api] above)." >&2
    exit 1
  fi
  sleep 1
done
echo "API ready: http://127.0.0.1:$API_PORT  Emails (Mailpit): http://localhost:8025"

pnpm --filter admin start > >(label admin) 2>&1 &
pids+=($!)
pnpm --filter storefront start > >(label storefront) 2>&1 &
pids+=($!)
echo "Admin: http://localhost:3001  Storefront: http://localhost:3002"

wait
