#!/usr/bin/env bash
# End-to-end check against a running hypha-host-axum (in-memory storage).
#
#   cargo run -p hypha-host-axum      # in one terminal
#   bash scripts/e2e.sh               # in another (Git Bash on Windows)
#
# Every step asserts the HTTP status and, for errors, the stable error code.
# Exits non-zero if any step fails. Uses only curl, grep and sed (no jq).
# Restart the server between runs: product handles must be unique.

set -u

BASE="${HYPHA_URL:-http://127.0.0.1:3000}"
JSON='content-type: application/json'
FAILED=0
BODY=''

# step NAME STATUS CODE_OR_DASH CURL_ARGS...
# Runs curl, checks the status and (unless "-") the "code" field. Leaves the body in $BODY.
step() {
    local name=$1 want_status=$2 want_code=$3
    shift 3
    local out status
    out=$(curl -s -w '\n%{http_code}' "$@")
    status=${out##*$'\n'}
    BODY=${out%$'\n'*}
    if [[ $status != "$want_status" ]]; then
        echo "FAIL $name: expected HTTP $want_status, got $status: $BODY"
        FAILED=$((FAILED + 1))
    elif [[ $want_code != - && $BODY != *"\"code\":\"$want_code\""* ]]; then
        echo "FAIL $name: expected code $want_code: $BODY"
        FAILED=$((FAILED + 1))
    else
        echo "ok   $name"
    fi
}

# contains NAME TEXT: asserts the last response body contains TEXT.
contains() {
    if [[ $BODY == *"$2"* ]]; then
        echo "ok   $1"
    else
        echo "FAIL $1: expected body to contain $2: $BODY"
        FAILED=$((FAILED + 1))
    fi
}

# The top-level "id" of the last response (always the first "id" key).
id() { sed -E 's/^[^"]*"id":"([^"]+)".*/\1/' <<<"$BODY"; }
# The first variant's id of the last product response.
variant() { sed -E 's/.*"variants":\[[^]]*"id":"(var_[0-9a-f]+)".*/\1/' <<<"$BODY"; }

if ! curl -s -o /dev/null "$BASE/"; then
    echo "No server at $BASE. Start it with: cargo run -p hypha-host-axum"
    exit 2
fi

echo "## happy path"
step "create mug" 201 - -X POST "$BASE/products" -H "$JSON" \
    -d '{"handle":"mug","title":"Mug","price":{"amount":1000,"currency":"EUR"}}'
if [[ $BODY == *'"code":"handle_taken"'* ]]; then
    echo "The server still has data from a previous run. Restart it and try again."
    exit 2
fi
MUG=$(id)
MUG_VAR=$(variant)
step "create cup" 201 - -X POST "$BASE/products" -H "$JSON" \
    -d '{"handle":"cup","title":"Cup","price":{"amount":250,"currency":"EUR"}}'
CUP=$(id)
CUP_VAR=$(variant)
step "activate mug" 200 - -X PATCH "$BASE/products/$MUG/status" -H "$JSON" -d '{"status":"active"}'
contains "mug is active" '"status":"active"'
step "activate cup" 200 - -X PATCH "$BASE/products/$CUP/status" -H "$JSON" -d '{"status":"active"}'

step "create market" 201 - -X POST "$BASE/markets" -H "$JSON" \
    -d '{"name":"Eurozone","countries":["HR","DE"],"currency":"EUR"}'
MARKET=$(id)
step "create price list (-10%, cup fixed at 200)" 201 - -X POST "$BASE/price-lists" -H "$JSON" \
    -d "{\"currency\":\"EUR\",\"adjustment\":-1000,\"fixed_prices\":[{\"variant\":\"$CUP_VAR\",\"price\":{\"amount\":200,\"currency\":\"EUR\"}}]}"
LIST=$(id)
step "create catalog" 201 - -X POST "$BASE/catalogs" -H "$JSON" \
    -d "{\"market\":\"$MARKET\",\"products\":[\"$MUG\",\"$CUP\"]}"
CATALOG=$(id)
step "assign price list" 200 - -X PUT "$BASE/catalogs/$CATALOG/price-list" -H "$JSON" \
    -d "{\"price_list\":\"$LIST\"}"

step "create cart" 201 - -X POST "$BASE/carts" -H "$JSON" -d "{\"market\":\"$MARKET\"}"
CART=$(id)
step "add 2 mugs" 200 - -X POST "$BASE/carts/$CART/lines" -H "$JSON" \
    -d "{\"variant\":\"$MUG_VAR\",\"quantity\":2}"
contains "mug unit price is 900 (1000 - 10%)" '"unit_price":{"amount":900,"currency":"EUR"}'
step "add 3 cups" 200 - -X POST "$BASE/carts/$CART/lines" -H "$JSON" \
    -d "{\"variant\":\"$CUP_VAR\",\"quantity\":3}"
contains "cart total is 2400" '"total":{"amount":2400,"currency":"EUR"},"fully_available":true'

step "archive mug" 200 - -X PATCH "$BASE/products/$MUG/status" -H "$JSON" -d '{"status":"archived"}'
step "get cart" 200 - "$BASE/carts/$CART"
contains "mug line is unavailable" '"status":"unavailable","reason_code":"product_not_active"'
contains "total only counts the cup" '"total":{"amount":600,"currency":"EUR"},"fully_available":false'

echo "## errors"
step "unknown route" 404 route_not_found "$BASE/nope"
step "wrong method" 405 method_not_allowed -X DELETE "$BASE/products/$MUG"
step "malformed json" 400 malformed_json -X POST "$BASE/markets" -H "$JSON" -d '{"name":'
step "missing field" 422 invalid_body -X POST "$BASE/markets" -H "$JSON" -d '{"name":"X"}'
step "unknown currency" 422 invalid_body -X POST "$BASE/markets" -H "$JSON" \
    -d '{"name":"UK","countries":["GB"],"currency":"GBP"}'
step "wrong id prefix" 400 invalid_path "$BASE/carts/$MUG"
step "unknown cart" 404 cart_not_found "$BASE/carts/cart_00000000000000000000000000000063"
step "duplicate handle" 409 handle_taken -X POST "$BASE/products" -H "$JSON" \
    -d '{"handle":"mug","title":"Mug 2","price":{"amount":1,"currency":"EUR"}}'
step "country in other market" 409 country_in_other_market -X POST "$BASE/markets" -H "$JSON" \
    -d '{"name":"DACH","countries":["AT","DE"],"currency":"EUR"}'
step "second catalog for market" 409 catalog_exists -X POST "$BASE/catalogs" -H "$JSON" \
    -d "{\"market\":\"$MARKET\"}"
step "create RSD price list" 201 - -X POST "$BASE/price-lists" -H "$JSON" -d '{"currency":"RSD"}'
RSD_LIST=$(id)
step "assign RSD list to EUR market" 422 price_list_currency_mismatch -X PUT \
    "$BASE/catalogs/$CATALOG/price-list" -H "$JSON" -d "{\"price_list\":\"$RSD_LIST\"}"
step "add archived mug" 422 product_not_active -X POST "$BASE/carts/$CART/lines" -H "$JSON" \
    -d "{\"variant\":\"$MUG_VAR\",\"quantity\":1}"
step "zero quantity" 422 invalid_quantity -X POST "$BASE/carts/$CART/lines" -H "$JSON" \
    -d "{\"variant\":\"$CUP_VAR\",\"quantity\":0}"
step "create draft product" 201 - -X POST "$BASE/products" -H "$JSON" \
    -d '{"handle":"draft","title":"Draft","price":{"amount":1,"currency":"EUR"}}'
step "draft -> archived" 422 invalid_status_transition -X PATCH "$BASE/products/$(id)/status" \
    -H "$JSON" -d '{"status":"archived"}'

echo
if ((FAILED > 0)); then
    echo "$FAILED step(s) failed"
    exit 1
fi
echo "all steps passed"
