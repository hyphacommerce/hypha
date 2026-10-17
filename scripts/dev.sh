#!/usr/bin/env bash
# Starts everything for local development: the compose services, the API on
# 127.0.0.1:3917 (what the apps' .env.example files name), the admin on :3001 and the
# storefront on :3002. Ctrl+C stops all three; the compose services keep running
# (`docker compose down` stops them).
#
#   pnpm dev                 # or: bash scripts/dev.sh
#   HYPHA_DEV_API_ONLY=1 pnpm dev
#   HYPHA_DEV_DB=hypha_demo HYPHA_OWNER_EMAIL=owner@example.com \
#     HYPHA_OWNER_PASSWORD=demo-owner-password pnpm dev   # then `pnpm seed`
#
# The API's HYPHA_SECRET_KEY is made once and kept in .dev-secret-key (ignored by git),
# since it seals 2FA secrets. Data lives in the hypha_preview database. On a fresh
# database, HYPHA_OWNER_EMAIL and HYPHA_OWNER_PASSWORD in the environment create the
# owner.
set -euo pipefail
cd "$(dirname "$0")/.."

API_PORT=3917

if ! docker info >/dev/null 2>&1; then
  echo "Docker isn't running: start Docker Desktop, then run this again." >&2
  exit 1
fi
docker compose up -d --wait

# A database of its own, apart from the one the tests use (`hypha`): `hypha_preview`,
# or HYPHA_DEV_DB (`hypha_demo` for the seeded demo store); DATABASE_URL wins.
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

pids=()
# cargo and pnpm run the servers as their children, so stop each whole tree.
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

# Each line of output is labelled with the process it came from (process substitution,
# so `$!` is the server's process rather than the labeller's).
label() { sed -u "s/^/[$1] /"; }

echo "Building and starting the API on :${API_PORT}…"
HYPHA_BIND="127.0.0.1:$API_PORT" \
  HYPHA_SECRET_KEY="$(cat .dev-secret-key)" \
  HYPHA_ALLOW_PRIVATE_CALLBACKS=true \
  cargo run -q -p hypha-host-axum > >(label api) 2>&1 &
pids+=($!)

until curl -sf "http://127.0.0.1:$API_PORT/health" >/dev/null; do
  if ! kill -0 "${pids[0]}" 2>/dev/null; then
    echo "The API stopped before it was ready (see [api] above)." >&2
    exit 1
  fi
  sleep 1
done
echo "API ready: http://127.0.0.1:$API_PORT"

if [ -z "${HYPHA_DEV_API_ONLY:-}" ]; then
  pnpm --filter admin dev > >(label admin) 2>&1 &
  pids+=($!)
  pnpm --filter storefront dev > >(label storefront) 2>&1 &
  pids+=($!)
  echo "Admin: http://localhost:3001  Storefront: http://localhost:3002"
fi

wait
