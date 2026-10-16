#!/usr/bin/env bash
# End-to-end check against a running hypha-host-axum on a fresh database, with file
# storage (`docker compose up -d`; see CLAUDE.md).
#
#   HYPHA_OWNER_EMAIL=owner@example.com HYPHA_OWNER_PASSWORD=e2e-owner-password \
#       cargo run -p hypha-host-axum  # in one terminal (PORT=... if 3000 is taken)
#   bash scripts/e2e.sh               # in another (Git Bash on Windows; HYPHA_URL=...)
#
# The script logs in as that owner (E2E_OWNER_EMAIL / E2E_OWNER_PASSWORD if you chose
# others) and sends the session with every step; the "auth" section checks the rest.
#
# Every step asserts the HTTP status and, for errors, the stable error code.
# Exits non-zero if any step fails. Uses only curl, grep, sed and head (no jq).
# Restart the server between runs: product handles must be unique.

set -u

BASE="${HYPHA_URL:-http://127.0.0.1:3000}"
OWNER_EMAIL="${E2E_OWNER_EMAIL:-owner@example.com}"
OWNER_PASSWORD="${E2E_OWNER_PASSWORD:-e2e-owner-password}"
JSON='content-type: application/json'
FAILED=0
BODY=''
# Credentials steps send (curl arguments): AUTH on admin routes (empty until the owner
# logs in), SF_AUTH (a publishable key) on /storefront/* routes, chosen by URL.
AUTH=()
SF_AUTH=()
CREDS=()

# pick_creds CURL_ARGS...: sets CREDS to SF_AUTH for a storefront URL, else AUTH.
pick_creds() {
    CREDS=(${AUTH[@]+"${AUTH[@]}"})
    local a
    for a in "$@"; do
        if [[ $a == "$BASE/storefront/"* ]]; then
            CREDS=(${SF_AUTH[@]+"${SF_AUTH[@]}"})
            return
        fi
    done
}

# step NAME STATUS CODE_OR_DASH CURL_ARGS...
# Runs curl, checks the status and (unless "-") the "code" field. Leaves the body in $BODY.
step() {
    local name=$1 want_status=$2 want_code=$3
    shift 3
    local out status
    pick_creds "$@"
    out=$(curl -s -w '\n%{http_code}' ${CREDS[@]+"${CREDS[@]}"} "$@")
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

# replayed NAME CURL_ARGS...: sends the request again and asserts the response carries
# `Idempotent-Replayed: true`.
replayed() {
    local name=$1
    shift
    pick_creds "$@"
    if curl -s -o /dev/null -D - ${CREDS[@]+"${CREDS[@]}"} "$@" | grep -qi '^idempotent-replayed: true'; then
        echo "ok   $name"
    else
        echo "FAIL $name: no Idempotent-Replayed header"
        FAILED=$((FAILED + 1))
    fi
}

# fresh NAME CURL_ARGS...: asserts the response does NOT carry `Idempotent-Replayed`.
fresh() {
    local name=$1
    shift
    pick_creds "$@"
    if curl -s -o /dev/null -D - ${CREDS[@]+"${CREDS[@]}"} "$@" | grep -qi '^idempotent-replayed'; then
        echo "FAIL $name: unexpected Idempotent-Replayed header"
        FAILED=$((FAILED + 1))
    else
        echo "ok   $name"
    fi
}

# wait_for NAME URL TEXT: asks URL once a second, up to 20 times, until its body
# contains TEXT (for what the dispatcher and search sync do every 2 seconds). Leaves the
# body in $BODY.
wait_for() {
    local name=$1 url=$2 text=$3 n
    pick_creds "$url"
    for n in $(seq 1 20); do
        BODY=$(curl -s ${CREDS[@]+"${CREDS[@]}"} "$url")
        if [[ $BODY == *"$text"* ]]; then
            echo "ok   $name"
            return
        fi
        sleep 1
    done
    echo "FAIL $name: expected body to contain $text within 20 s: $BODY"
    FAILED=$((FAILED + 1))
}

# The top-level "id" of the last response (always the first "id" key).
id() { sed -E 's/^[^"]*"id":"([^"]+)".*/\1/' <<<"$BODY"; }
# The first address id of the last customer response.
address() { sed -E 's/.*"addresses":\[\{"id":"(addr_[0-9a-f]+)".*/\1/' <<<"$BODY"; }
# The Nth order line id (1-based) of the last order response.
ordl() { grep -o '"id":"ordl_[0-9a-f]*"' <<<"$BODY" | sed -n "${1}p" | sed -E 's/.*"(ordl_[0-9a-f]+)"/\1/'; }
# The "customer" of the last order response.
customer() { sed -E 's/.*"customer":"(cus_[0-9a-f]+)".*/\1/' <<<"$BODY"; }
# The "token" of the last response (a session, a challenge or a new key).
token() { sed -E 's/.*"token":"(hypha_[a-z]+_[0-9a-f]+)".*/\1/' <<<"$BODY"; }
# The last shipping option's id in the last zone response.
shipopt() { sed -E 's/.*"id":"(shipopt_[0-9a-f]+)".*/\1/' <<<"$BODY"; }
# The first shipping group's id of the last cart response.
shgrp() { sed -E 's/.*"shipping_groups":\[\{"id":"(shgrp_[0-9a-f]+)".*/\1/' <<<"$BODY"; }
# The first variant's id of the last product response.
variant() { sed -E 's/.*"variants":\[[^]]*"id":"(var_[0-9a-f]+)".*/\1/' <<<"$BODY"; }

if ! curl -s -o /dev/null "$BASE/"; then
    echo "No server at $BASE. Start it with: cargo run -p hypha-host-axum"
    exit 2
fi

echo "## logging in"
step "health needs no credentials" 200 - "$BASE/health"
step "nor does the admin API's OpenAPI document" 200 - "$BASE/admin/openapi.json"
contains "it is OpenAPI 3.1" '"openapi": "3.1.0"'
step "or the storefront's" 200 - "$BASE/storefront/openapi.json"
contains "with the storefront's title" '"title": "Hypha Storefront API"'
step "anything else does" 401 unauthenticated "$BASE/admin/store"
step "owner logs in" 200 - -X POST "$BASE/admin/auth/login" -H "$JSON" \
    -d "{\"email\":\"$OWNER_EMAIL\",\"password\":\"$OWNER_PASSWORD\"}"
if [[ $BODY != *'"status":"authenticated"'* ]]; then
    echo "Cannot log in as $OWNER_EMAIL. Start the server with HYPHA_OWNER_EMAIL and"
    echo "HYPHA_OWNER_PASSWORD set to the same values (see the top of this script)."
    exit 2
fi
SESSION=$(token)
AUTH=(-H "authorization: Bearer $SESSION")
step "a publishable key for the storefront steps" 201 - -X POST "$BASE/admin/api-keys" -H "$JSON" \
    -d '{"kind":"publishable","name":"e2e storefront"}'
SF_AUTH=(-H "x-publishable-key: $(token)")

echo
echo "## happy path"
step "health" 200 - "$BASE/health"
step "the store this deployment serves" 200 - "$BASE/admin/store"
contains "store has a store_ ID" '"id":"store_'
step "rename the store" 200 - -X PATCH "$BASE/admin/store" -H "$JSON" -d '{"name":"E2E Shop"}'
contains "store has its new name" '"name":"E2E Shop"'
step "a blank store name" 422 store_name_empty -X PATCH "$BASE/admin/store" -H "$JSON" -d '{"name":" "}'
step "create mug" 201 - -X POST "$BASE/admin/products" -H "$JSON" \
    -d '{"handle":"mug","title":"Mug","price":{"amount":1000,"currency":"EUR"}}'
if [[ $BODY == *'"code":"handle_taken"'* ]]; then
    echo "The server still has data from a previous run. Restart it and try again."
    exit 2
fi
MUG=$(id)
MUG_VAR=$(variant)
step "create cup" 201 - -X POST "$BASE/admin/products" -H "$JSON" \
    -d '{"handle":"cup","title":"Cup","price":{"amount":250,"currency":"EUR"}}'
CUP=$(id)
CUP_VAR=$(variant)
step "activate mug" 200 - -X PATCH "$BASE/admin/products/$MUG/status" -H "$JSON" -d '{"status":"active"}'
contains "mug is active" '"status":"active"'
step "activate cup" 200 - -X PATCH "$BASE/admin/products/$CUP/status" -H "$JSON" -d '{"status":"active"}'
step "list products, one per page" 200 - "$BASE/admin/products?limit=1"
contains "newest first, with more after it" "\"items\":[{\"id\":\"$CUP\""
contains "with a cursor for the next page" '"has_more":true,"next_cursor":"'
NEXT=$(sed -E 's/.*"next_cursor":"([^"]+)".*/\1/' <<<"$BODY")
step "the next page" 200 - "$BASE/admin/products?limit=1&cursor=$NEXT"
contains "is the mug" "\"id\":\"$MUG\""
contains "as a summary" '"variant_count":1,"price_ranges":[{"min":'
step "active products" 200 - "$BASE/admin/products?status=active"
step "products by text (design note 0019)" 200 - "$BASE/admin/products?q=MU"
contains "finds the mug by its title" "\"items\":[{\"id\":\"$MUG\""
step "text too long" 422 field_too_long "$BASE/admin/products?q=$(printf 'x%.0s' {1..256})"
step "a page too large" 422 invalid_limit "$BASE/admin/products?limit=500"
step "an unknown filter" 400 invalid_query "$BASE/admin/products?sort=title"
step "a cursor from another list" 400 invalid_cursor "$BASE/admin/products?cursor=ord_00000000000000000000000000000000"

# product editing (design note 0014), on a product of its own
step "create shirt" 201 - -X POST "$BASE/admin/products" -H "$JSON" \
    -d '{"handle":"shirt","title":"Shirt","price":{"amount":1500,"currency":"EUR"}}'
SHIRT=$(id)
SHIRT_S=$(variant)
step "rename it" 200 - -X PATCH "$BASE/admin/products/$SHIRT" -H "$JSON" \
    -d '{"title":"Linen shirt","handle":"linen-shirt"}'
contains "with its new handle" '"handle":"linen-shirt","title":"Linen shirt"'
step "describe it (design note 0019)" 200 - -X PATCH "$BASE/admin/products/$SHIRT" -H "$JSON" \
    -d '{"description":"<p>Soft linen</p><script>x()</script>","vendor":"Acme","product_type":"Shirt","tags":["Summer","summer","Linen"]}'
contains "sanitised, with its tags deduplicated" '"description":"<p>Soft linen</p>","vendor":"Acme","product_type":"Shirt","tags":["Summer","Linen"]'
step "too many tags" 422 too_many_tags -X PATCH "$BASE/admin/products/$SHIRT" -H "$JSON" \
    -d "{\"tags\":[$(printf '"t%d",' $(seq 0 249))\"t250\"]}"
step "a taken handle" 409 handle_taken -X PATCH "$BASE/admin/products/$SHIRT" -H "$JSON" -d '{"handle":"mug"}'
step "a title can't be cleared" 422 - -X PATCH "$BASE/admin/products/$SHIRT" -H "$JSON" -d '{"title":null}'
step "give it a size" 200 - -X PUT "$BASE/admin/products/$SHIRT/options" -H "$JSON" \
    -d '{"options":[{"name":"Size","values":[{"label":"S"},{"label":"M"}]}]}'
contains "its variant is S" '"title":"S"'
SIZE=$(sed -E 's/.*"options":\[\{"id":"(opt_[0-9a-f]+)".*/\1/' <<<"$BODY")
SIZE_S=$(sed -E 's/.*"id":"(optval_[0-9a-f]+)","label":"S".*/\1/' <<<"$BODY")
SIZE_M=$(sed -E 's/.*"id":"(optval_[0-9a-f]+)","label":"M".*/\1/' <<<"$BODY")
ADD_M="{\"sku\":\"SHIRT-M\",\"price\":{\"amount\":1600,\"currency\":\"EUR\"},\"selections\":[\"$SIZE_M\"]}"
step "add the M variant" 201 - -X POST "$BASE/admin/products/$SHIRT/variants" -H "$JSON" -d "$ADD_M"
SHIRT_M=$(sed -E 's/.*"id":"(var_[0-9a-f]+)","price":\{[^}]*\},"sku":"SHIRT-M".*/\1/' <<<"$BODY")
step "M twice" 409 duplicate_option_combination -X POST "$BASE/admin/products/$SHIRT/variants" -H "$JSON" -d "$ADD_M"
step "remove the M variant" 200 - -X DELETE "$BASE/admin/products/$SHIRT/variants/$SHIRT_M"
step "and add it back" 201 - -X POST "$BASE/admin/products/$SHIRT/variants" -H "$JSON" -d "$ADD_M"
SHIRT_M=$(sed -E 's/.*"id":"(var_[0-9a-f]+)","price":\{[^}]*\},"sku":"SHIRT-M".*/\1/' <<<"$BODY")
step "the S variant's SKU and price" 200 - -X PATCH "$BASE/admin/products/$SHIRT/variants/$SHIRT_S" -H "$JSON" \
    -d '{"sku":"SHIRT-S","price":{"amount":1400,"currency":"EUR"}}'
contains "are stored" '"price":{"amount":1400,"currency":"EUR"},"sku":"SHIRT-S"'
step "a SKU another variant has" 409 sku_taken -X PATCH "$BASE/admin/products/$SHIRT/variants/$SHIRT_S" -H "$JSON" \
    -d '{"sku":"SHIRT-M"}'
ONLY_S="{\"options\":[{\"id\":\"$SIZE\",\"name\":\"Size\",\"values\":[{\"id\":\"$SIZE_S\",\"label\":\"S\"}]}]"
step "removing M while a variant uses it" 409 option_value_in_use -X PUT "$BASE/admin/products/$SHIRT/options" \
    -H "$JSON" -d "$ONLY_S}"
contains "names that variant" "\"details\":{\"variants\":[\"$SHIRT_M\"]}"
step "removing M with its variant" 200 - -X PUT "$BASE/admin/products/$SHIRT/options" \
    -H "$JSON" -d "$ONLY_S,\"remove_variants\":[\"$SHIRT_M\"]}"
step "the last variant stays" 409 no_variants -X DELETE "$BASE/admin/products/$SHIRT/variants/$SHIRT_S"
step "an unknown variant" 404 variant_not_found \
    -X DELETE "$BASE/admin/products/$SHIRT/variants/var_00000000000000000000000000000000"

# media (design note 0013): the file goes straight to storage with the signed request,
# so that PUT is a plain curl (no admin credentials; S3 checks its own signature).
PNG_FILE=$(mktemp)
{ printf '\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR\x00\x00\x00\x03\x00\x00\x00\x02\x08\x06\x00\x00\x00'; head -c 71 /dev/zero; } > "$PNG_FILE"
step "start an upload" 201 - -X POST "$BASE/admin/assets" -H "$JSON" \
    -d '{"filename":"Shirt front.png","mime_type":"image/png","byte_size":100}'
contains "the asset is pending" '"status":"pending"'
contains "the upload is signed for its type" '"name":"content-type","value":"image/png"'
ASSET=$(sed -E 's/.*"asset":\{"id":"([^"]+)".*/\1/' <<<"$BODY")
UPLOAD_URL=$(sed -E 's/.*"upload":\{"method":"PUT","url":"([^"]+)".*/\1/' <<<"$BODY")
step "complete before uploading" 409 upload_missing -X POST "$BASE/admin/assets/$ASSET/complete"
STORED=$(curl -s -o /dev/null -w '%{http_code}' -X PUT -H 'content-type: image/png' \
    -H 'cache-control: public, max-age=31536000, immutable' --data-binary "@$PNG_FILE" "$UPLOAD_URL")
if [[ $STORED == 200 ]]; then echo "ok   upload the file to storage"; else
    echo "FAIL upload the file to storage: HTTP $STORED"; FAILED=$((FAILED + 1)); fi
rm -f "$PNG_FILE"
step "complete the upload" 200 - -X POST "$BASE/admin/assets/$ASSET/complete"
contains "it is ready, with its dimensions" '"width":3,"height":2'
step "list images" 200 - "$BASE/admin/assets?kind=image"
contains "the asset is listed" "\"items\":[{\"id\":\"$ASSET\""
step "set its alt text" 200 - -X PATCH "$BASE/admin/assets/$ASSET" -H "$JSON" -d '{"alt":"A blue shirt"}'
contains "the alt text is stored" '"alt":"A blue shirt"'
step "an SVG is refused" 422 unsupported_media_type -X POST "$BASE/admin/assets" -H "$JSON" \
    -d '{"filename":"logo.svg","mime_type":"image/svg+xml","byte_size":10}'
step "attach it to the shirt" 200 - -X PUT "$BASE/admin/products/$SHIRT/media" -H "$JSON" \
    -d "{\"assets\":[\"$ASSET\"]}"
contains "with a URL and its alt text" "\"media\":[{\"asset_id\":\"$ASSET\",\"kind\":\"image\""
contains "and its alt text" '"alt":"A blue shirt","width":3,"height":2}]'
step "the S variant shows it" 200 - -X PUT "$BASE/admin/products/$SHIRT/variants/$SHIRT_S/media" -H "$JSON" \
    -d "{\"asset\":\"$ASSET\"}"
contains "by its asset" "\"media_asset_id\":\"$ASSET\""
step "the list features it" 200 - "$BASE/admin/products?limit=1"
contains "on the shirt's summary" "\"featured_media\":{\"asset_id\":\"$ASSET\""
step "the asset is used by the shirt" 200 - "$BASE/admin/assets/$ASSET/usages"
contains "as its featured item" "{\"owner\":{\"kind\":\"product\",\"id\":\"$SHIRT\"},\"role\":\"featured\"}"
step "an asset in use isn't deleted" 409 asset_in_use -X DELETE "$BASE/admin/assets/$ASSET"
contains "the usages are in the details" '"has_more":false'
step "a missing asset isn't attached" 422 asset_not_found -X PUT "$BASE/admin/products/$SHIRT/media" -H "$JSON" \
    -d '{"assets":["asset_00000000000000000000000000000000"]}'
step "detach it" 200 - -X PUT "$BASE/admin/products/$SHIRT/media" -H "$JSON" -d '{"assets":[]}'
contains "the variant shows nothing now" '"media_asset_id":null'
step "delete the asset" 204 - -X DELETE "$BASE/admin/assets/$ASSET"
step "it is gone" 404 asset_not_found "$BASE/admin/assets/$ASSET"

step "create market" 201 - -X POST "$BASE/admin/markets" -H "$JSON" \
    -d '{"name":"Eurozone","countries":["HR","DE"],"currency":"EUR"}'
MARKET=$(id)
contains "prices exclude tax by default" '"prices_include_tax":false'
step "prices include tax" 200 - -X PATCH "$BASE/admin/markets/$MARKET" -H "$JSON" -d '{"prices_include_tax":true}'
contains "market now includes tax" '"prices_include_tax":true'
step "back to excluding tax" 200 - -X PATCH "$BASE/admin/markets/$MARKET" -H "$JSON" -d '{"prices_include_tax":false}'
step "sell to Slovenia too" 200 - -X POST "$BASE/admin/markets/$MARKET/countries" -H "$JSON" -d '{"country":"SI"}'
contains "Slovenia added" '"countries":["HR","DE","SI"]'
step "stop selling to Slovenia" 200 - -X DELETE "$BASE/admin/markets/$MARKET/countries/SI"
contains "Slovenia removed" '"countries":["HR","DE"]'
step "create price list (-10%)" 201 - -X POST "$BASE/admin/price-lists" -H "$JSON" \
    -d '{"currency":"EUR","adjustment_bps":-1000}'
LIST=$(id)
step "fix cup price at 200" 204 - -X PUT "$BASE/admin/price-lists/$LIST/fixed-prices" -H "$JSON" \
    -d "{\"fixed_prices\":[{\"variant\":\"$CUP_VAR\",\"price\":{\"amount\":200,\"currency\":\"EUR\"}}]}"
step "create catalog" 201 - -X POST "$BASE/admin/catalogs" -H "$JSON" -d "{\"market\":\"$MARKET\"}"
CATALOG=$(id)
step "add mug and cup to catalog" 204 - -X POST "$BASE/admin/catalogs/$CATALOG/products" -H "$JSON" \
    -d "{\"products\":[\"$MUG\",\"$CUP\"]}"
step "assign price list" 200 - -X PUT "$BASE/admin/catalogs/$CATALOG/price-list" -H "$JSON" \
    -d "{\"price_list\":\"$LIST\"}"

step "create cart" 201 - -X POST "$BASE/storefront/carts" -H "$JSON" -d '{"country":"HR"}'
contains "cart ships to Croatia" '"country":"HR"'
contains "cart names its market and currency" "\"market\":\"$MARKET\",\"currency\":\"EUR\""
CART=$(id)
step "add 2 mugs" 200 - -X POST "$BASE/storefront/carts/$CART/lines" -H "$JSON" \
    -d "{\"variant\":\"$MUG_VAR\",\"quantity\":2}"
contains "mug unit price is 900 (1000 - 10%)" '"unit_price":{"amount":900,"currency":"EUR"}'
contains "mug line is named" '"product_title":"Mug","variant_title":null,"sku":null,"status":"priced"'
step "add 3 cups" 200 - -X POST "$BASE/storefront/carts/$CART/lines" -H "$JSON" \
    -d "{\"variant\":\"$CUP_VAR\",\"quantity\":3}"
contains "cart total is 2400" '"total":{"amount":2400,"currency":"EUR"},"fully_available":true'

echo "## idempotency keys"
fresh "a first request isn't marked replayed" -X POST "$BASE/storefront/carts" -H "$JSON" \
    -H 'idempotency-key: cart-0' -d '{"country":"HR"}'
step "create cart with a key" 201 - -X POST "$BASE/storefront/carts" -H "$JSON" -H 'idempotency-key: cart-1' \
    -d '{"country":"HR"}'
KEYED=$(id)
step "retry it" 201 - -X POST "$BASE/storefront/carts" -H "$JSON" -H 'idempotency-key: cart-1' \
    -d '{"country":"HR"}'
contains "the retry returns the first cart" "\"id\":\"$KEYED\""
replayed "the retry is marked replayed" -X POST "$BASE/storefront/carts" -H "$JSON" \
    -H 'idempotency-key: cart-1' -d '{"country":"HR"}'
step "same key, different body" 422 idempotency_key_reused -X POST "$BASE/storefront/carts" -H "$JSON" \
    -H 'idempotency-key: cart-1' -d '{"country":"DE"}'
step "a key with a space" 422 invalid_idempotency_key -X POST "$BASE/storefront/carts" -H "$JSON" \
    -H 'idempotency-key: two words' -d '{"country":"HR"}'
step "add 2 mugs with a key" 200 - -X POST "$BASE/storefront/carts/$KEYED/lines" -H "$JSON" \
    -H 'idempotency-key: line-1' -d "{\"variant\":\"$MUG_VAR\",\"quantity\":2}"
step "retry the add" 200 - -X POST "$BASE/storefront/carts/$KEYED/lines" -H "$JSON" \
    -H 'idempotency-key: line-1' -d "{\"variant\":\"$MUG_VAR\",\"quantity\":2}"
contains "the retry didn't add the mugs again" '"quantity":2,'
replayed "the add retry is marked replayed" -X POST "$BASE/storefront/carts/$KEYED/lines" -H "$JSON" \
    -H 'idempotency-key: line-1' -d "{\"variant\":\"$MUG_VAR\",\"quantity\":2}"

echo "## shipping (design note 0015)"
step "a zone for Croatia and Germany" 201 - -X POST "$BASE/admin/shipping-zones" -H "$JSON" \
    -d '{"name":"Eurozone","countries":["HR","DE"]}'
ZONE=$(id)
OPTIONS="$BASE/admin/shipping-zones/$ZONE/options"
step "a country in two zones" 409 country_in_other_zone -X POST "$BASE/admin/shipping-zones" -H "$JSON" \
    -d '{"name":"Again","countries":["HR"]}'
step "standard: 4.90, free from 10 EUR" 201 - -X POST "$OPTIONS" -H "$JSON" \
    -d '{"name":"Standard","code":"standard","rate":{"type":"builtin","basis":"subtotal","tables":[{"currency":"EUR","tiers":[{"up_to":1000,"amount":490},{"amount":0}]}]},"estimate":{"min_days":2,"max_days":4}}'
STANDARD=$(shipopt)
step "a last tier with a bound" 422 invalid_shipping_rate -X POST "$OPTIONS" -H "$JSON" \
    -d '{"name":"Bad","code":"bad","rate":{"type":"builtin","basis":"weight","tables":[{"currency":"EUR","tiers":[{"up_to":1000,"amount":490}]}]}}'
step "a second option with the same code" 422 shipping_option_code_taken -X POST "$OPTIONS" -H "$JSON" \
    -d '{"name":"Again","code":"standard","rate":{"type":"builtin","basis":"flat","tables":[{"currency":"EUR","tiers":[{"amount":100}]}]}}'
step "an app for rates" 201 - -X POST "$BASE/admin/apps" -H "$JSON" -d '{"key":"app_courier","name":"Courier"}'
contains "its secret is shown once" '"secret":"hypha_as_'
step "its rates, unreachable" 201 - -X POST "$BASE/admin/shipping-providers" -H "$JSON" \
    -d '{"key":"app_courier","callback_url":"http://127.0.0.1:9/rates","timeout_ms":500}'
step "the same key again" 409 shipping_provider_exists -X POST "$BASE/admin/shipping-providers" -H "$JSON" \
    -d '{"key":"app_courier","callback_url":"http://127.0.0.1:9/rates"}'
step "rates for an app that doesn't exist" 422 provider_app_not_found -X POST "$BASE/admin/shipping-providers" \
    -H "$JSON" -d '{"key":"app_ghost","callback_url":"https://rates.example.com"}'
step "a local host hiding another behind credentials" 422 invalid_callback_url -X POST "$BASE/admin/shipping-providers" \
    -H "$JSON" -d '{"key":"app_sneaky","callback_url":"http://localhost:80@evil.example/rates"}'
step "a callback over plain HTTP elsewhere" 422 invalid_callback_url -X POST "$BASE/admin/shipping-providers" -H "$JSON" \
    -d '{"key":"app_plain","callback_url":"http://rates.example.com"}'
step "get the provider" 200 - "$BASE/admin/shipping-providers/app_courier"
contains "without a secret" '"key":"app_courier","callback_url":"http://127.0.0.1:9/rates","timeout_ms":500,"services":[]}'
step "its services, unreachable" 502 shipping_provider_failed "$BASE/admin/shipping-providers/app_courier/services"
step "express by the courier" 201 - -X POST "$OPTIONS" -H "$JSON" \
    -d '{"name":"Express","code":"express","rate":{"type":"provider","provider":"app_courier","service":"express"}}'
step "post, the backup" 201 - -X POST "$OPTIONS" -H "$JSON" \
    -d '{"name":"Post","code":"post","is_backup":true,"rate":{"type":"builtin","basis":"flat","tables":[{"currency":"EUR","tiers":[{"amount":300}]}]}}'
POST_OPTION=$(shipopt)
step "list zones" 200 - "$BASE/admin/shipping-zones"
contains "with three options" '"code":"standard"'

step "a shipping cart" 201 - -X POST "$BASE/storefront/carts" -H "$JSON" -d '{"country":"HR"}'
SHIP_CART=$(id)
SHIP="$BASE/storefront/carts/$SHIP_CART"
step "add a mug" 200 - -X POST "$SHIP/lines" -H "$JSON" -d "{\"variant\":\"$MUG_VAR\",\"quantity\":1}"
contains "it ships in one group, not chosen yet" '"selected":null,"problem":null}],"shipping_ready":false'
SHGRP=$(shgrp)
step "offers need an address" 409 shipping_address_required "$SHIP/shipping-options"
step "an address in Slovenia" 422 address_country_mismatch -X PUT "$SHIP/shipping-address" -H "$JSON" \
    -d '{"address":{"last_name":"Novak","line1":"Slovenska 1","city":"Ljubljana","country":"SI"}}'
step "ship to Zagreb" 200 - -X PUT "$SHIP/shipping-address" -H "$JSON" \
    -d '{"address":{"first_name":"Ana","last_name":"Horvat","line1":"Ilica 1","city":"Zagreb","postal_code":"10000","country":"HR"}}'
contains "the cart has the address" '"city":"Zagreb"'
step "the offers" 200 - "$SHIP/shipping-options"
contains "standard costs 4.90 under 10 EUR" '"code":"standard","name":"Standard","amount":{"amount":490,"currency":"EUR"},"estimate":{"min_days":2,"max_days":4}'
contains "the courier is down, and express says why" '"name":"Express","reason":"provider_failed"'
contains "standard is offered, so the backup isn't needed" '"name":"Post","reason":"backup_not_needed"'
step "the backup can't be picked" 409 shipping_option_not_offered -X PUT "$SHIP/shipping/$SHGRP" -H "$JSON" \
    -d "{\"option\":\"$POST_OPTION\"}"
contains "and says why" '"details":{"reason":"backup_not_needed"}'
step "an unknown option" 422 shipping_option_unknown -X PUT "$SHIP/shipping/$SHGRP" -H "$JSON" \
    -d '{"option":"shipopt_00000000000000000000000000000000"}'
step "an unknown group" 404 shipping_group_not_found -X PUT "$SHIP/shipping/shgrp_00000000000000000000000000000000" \
    -H "$JSON" -d "{\"option\":\"$STANDARD\"}"
step "an email" 200 - -X PUT "$SHIP/email" -H "$JSON" -d '{"email":"shipping@example.hr"}'
step "checkout before choosing" 409 shipping_option_required -X POST "$SHIP/checkout" -H "$JSON" -d '{}'
step "pick standard" 200 - -X PUT "$SHIP/shipping/$SHGRP" -H "$JSON" -d "{\"option\":\"$STANDARD\"}"
contains "shipping is in the total" '"subtotal":{"amount":900,"currency":"EUR"},"shipping_total":{"amount":490,"currency":"EUR"},"tax_total":{"amount":0,"currency":"EUR"},"total":{"amount":1390'
contains "and ready" '"shipping_ready":true'

echo "## checkout"
CHECKOUT="$BASE/storefront/carts/$KEYED/checkout"
step "checkout an unknown cart" 404 cart_not_found -X POST \
    "$BASE/storefront/carts/cart_00000000000000000000000000000063/checkout" -H "$JSON" -d '{}'
step "checkout with an unknown field" 422 invalid_body -X POST "$CHECKOUT" -H "$JSON" \
    -d '{"total":1}'
step "ship the keyed cart to Zagreb" 200 - -X PUT "$BASE/storefront/carts/$KEYED/shipping-address" -H "$JSON" \
    -d '{"address":{"last_name":"Horvat","line1":"Ilica 1","city":"Zagreb","country":"HR"}}'
step "standard, free from 10 EUR" 200 - -X PUT "$BASE/storefront/carts/$KEYED/shipping/$(shgrp)" -H "$JSON" \
    -d "{\"option\":\"$STANDARD\"}"
step "checkout without an email" 409 cart_email_missing -X POST "$CHECKOUT" -H "$JSON" -d '{}'
step "give the cart an email" 200 - -X PUT "$BASE/storefront/carts/$KEYED/email" -H "$JSON" \
    -d '{"email":"checkout@example.hr"}'
step "checkout at a stale total" 409 total_changed -X POST "$CHECKOUT" -H "$JSON" \
    -d '{"expected_total":{"amount":1700,"currency":"EUR"}}'
contains "the error names both totals" '"details":{"actual":{"amount":1800,"currency":"EUR"},"expected":{"amount":1700,"currency":"EUR"}}'
step "checkout" 201 - -X POST "$CHECKOUT" -H "$JSON" -H 'idempotency-key: order-1' \
    -d '{"expected_total":{"amount":1800,"currency":"EUR"}}'
ORDER=$(id)
contains "the order is #1001" '"number":1001'
contains "it is pending and unfulfilled" '"financial_status":"pending","fulfillment_status":"unfulfilled"'
contains "it copies the line" '"product_title":"Mug","variant_title":null,"sku":null,"unit_price":{"amount":900,"currency":"EUR"}'
contains "its total is 1800" '"total":{"amount":1800,"currency":"EUR"},"lines"'
step "retry the checkout" 201 - -X POST "$CHECKOUT" -H "$JSON" -H 'idempotency-key: order-1' \
    -d '{"expected_total":{"amount":1800,"currency":"EUR"}}'
contains "the retry returns the same order" "\"id\":\"$ORDER\""
replayed "a retried checkout replays the order" -X POST "$CHECKOUT" -H "$JSON" \
    -H 'idempotency-key: order-1' -d '{"expected_total":{"amount":1800,"currency":"EUR"}}'
step "retry, re-serialized (spaces, key order)" 201 - -X POST "$CHECKOUT" -H "$JSON" \
    -H 'idempotency-key: order-1' -d '{ "expected_total" : { "currency" : "EUR", "amount" : 1800 } }'
contains "is the same request" "\"id\":\"$ORDER\""
step "same key, different total" 422 idempotency_key_reused -X POST "$CHECKOUT" -H "$JSON" \
    -H 'idempotency-key: order-1' -d '{"expected_total":{"amount":1801,"currency":"EUR"}}'
step "a cart key reused to add a line" 422 idempotency_key_reused -X POST "$BASE/storefront/carts/$CART/lines" \
    -H "$JSON" -H 'idempotency-key: cart-1' -d "{\"variant\":\"$MUG_VAR\",\"quantity\":1}"
step "checkout again without the key" 409 cart_completed -X POST "$CHECKOUT" -H "$JSON" -d '{}'
contains "the error names the order" "\"details\":{\"order\":\"$ORDER\"}"
step "get the checked-out cart" 200 - "$BASE/storefront/carts/$KEYED"
contains "the cart names its order" "\"order\":\"$ORDER\""
step "a completed cart takes no lines" 409 cart_completed -X POST "$BASE/storefront/carts/$KEYED/lines" \
    -H "$JSON" -d "{\"variant\":\"$MUG_VAR\",\"quantity\":1}"
step "get the order" 200 - "$BASE/admin/orders/$ORDER"
contains "it has a placed_at" '"placed_at":"20'
step "list orders" 200 - "$BASE/admin/orders?limit=1"
contains "the order is listed first" "\"items\":[{\"id\":\"$ORDER\""
step "mark it paid" 200 - -X POST "$BASE/admin/orders/$ORDER/paid"
contains "it is paid" '"financial_status":"paid"'
step "mark it paid again" 200 - -X POST "$BASE/admin/orders/$ORDER/paid"
contains "still paid" '"financial_status":"paid"'
step "an unknown order" 404 order_not_found "$BASE/admin/orders/ord_00000000000000000000000000000000"
step "an order ID of the wrong kind" 400 invalid_path "$BASE/admin/orders/$CART"
step "a new cart" 201 - -X POST "$BASE/storefront/carts" -H "$JSON" -d '{"country":"HR"}'
contains "an open cart has no order" '"order":null'
step "checkout an empty cart" 409 cart_empty -X POST "$BASE/storefront/carts/$(id)/checkout" -H "$JSON" -d '{}'
step "checkout the shipping cart" 201 - -X POST "$SHIP/checkout" -H "$JSON" -d '{}'
contains "the order charges shipping" '"shipping_total":{"amount":490,"currency":"EUR"},"tax_total":{"amount":0,"currency":"EUR"},"total":{"amount":1390'
contains "it copies the address" '"shipping_address":{"first_name":"Ana","last_name":"Horvat"'
contains "and the shipping line" '"code":"standard","name":"Standard","provider":null,"service":null,"reference":null,"amount":{"amount":490,"currency":"EUR"},"estimate":{"min_days":2,"max_days":4}'

step "archive mug" 200 - -X PATCH "$BASE/admin/products/$MUG/status" -H "$JSON" -d '{"status":"archived"}'
step "get cart" 200 - "$BASE/storefront/carts/$CART"
contains "mug line is unavailable" '"status":"unavailable","reason_code":"product_not_active"'
contains "unavailable mug keeps its name" '"product_title":"Mug","variant_title":null,"sku":null,"status":"unavailable"'
contains "total only counts the cup" '"total":{"amount":600,"currency":"EUR"},"fully_available":false'

step "remove cup's fixed price" 204 - -X DELETE "$BASE/admin/price-lists/$LIST/fixed-prices" -H "$JSON" \
    -d "{\"variants\":[\"$CUP_VAR\"]}"
step "get cart" 200 - "$BASE/storefront/carts/$CART"
contains "cup falls back to 250 - 10% = 225" '"unit_price":{"amount":225,"currency":"EUR"},"total":{"amount":675'
step "remove cup from catalog" 204 - -X DELETE "$BASE/admin/catalogs/$CATALOG/products" -H "$JSON" \
    -d "{\"products\":[\"$CUP\"]}"
step "get cart" 200 - "$BASE/storefront/carts/$CART"
contains "cup line is unavailable" '"status":"unavailable","reason_code":"not_available_in_market"'

echo "## tax"
step "create book" 201 - -X POST "$BASE/admin/products" -H "$JSON" \
    -d '{"handle":"book","title":"Book","price":{"amount":1000,"currency":"EUR"}}'
BOOK=$(id)
BOOK_VAR=$(variant)
step "activate book" 200 - -X PATCH "$BASE/admin/products/$BOOK/status" -H "$JSON" -d '{"status":"active"}'
step "add book to catalog" 204 - -X POST "$BASE/admin/catalogs/$CATALOG/products" -H "$JSON" \
    -d "{\"products\":[\"$BOOK\"]}"
step "create tax class" 201 - -X POST "$BASE/admin/tax-classes" -H "$JSON" -d '{"name":"Books"}'
BOOKS=$(id)
step "put book in it" 204 - -X PUT "$BASE/admin/tax-assignments" -H "$JSON" \
    -d "{\"products\":[\"$BOOK\"],\"tax_class\":\"$BOOKS\"}"
step "create Croatian tax region (25%)" 201 - -X POST "$BASE/admin/tax-regions" -H "$JSON" \
    -d '{"country":"HR","default_rate_ppm":250000}'
REGION=$(id)
step "books pay 5% in Croatia" 200 - -X PUT "$BASE/admin/tax-regions/$REGION/class-rates/$BOOKS" \
    -H "$JSON" -d '{"rate_ppm":50000}'
contains "class rate is stored" "\"class_rates\":[{\"tax_class\":\"$BOOKS\",\"rate_ppm\":50000}]"
step "get tax region" 200 - "$BASE/admin/tax-regions/$REGION"
step "create taxed cart" 201 - -X POST "$BASE/storefront/carts" -H "$JSON" -d '{"country":"HR"}'
TAX_CART=$(id)
step "add 2 books" 200 - -X POST "$BASE/storefront/carts/$TAX_CART/lines" -H "$JSON" \
    -d "{\"variant\":\"$BOOK_VAR\",\"quantity\":2}"
contains "book line pays 5% of 1800" "\"tax\":{\"rate_ppm\":50000,\"amount\":{\"amount\":90,\"currency\":\"EUR\"},\"tax_class\":\"$BOOKS\"}"
contains "tax is added on top" '"taxes_included":false,"subtotal":{"amount":1800,"currency":"EUR"},"shipping_total":{"amount":0,"currency":"EUR"},"tax_total":{"amount":90,"currency":"EUR"},"total":{"amount":1890'
step "prices include tax" 200 - -X PATCH "$BASE/admin/markets/$MARKET" -H "$JSON" -d '{"prices_include_tax":true}'
step "get taxed cart" 200 - "$BASE/storefront/carts/$TAX_CART"
contains "tax is extracted (1800 x 5/105 = 85.7)" '"taxes_included":true,"subtotal":{"amount":1800,"currency":"EUR"},"shipping_total":{"amount":0,"currency":"EUR"},"tax_total":{"amount":86,"currency":"EUR"},"total":{"amount":1800'
step "remove books rate" 200 - -X DELETE "$BASE/admin/tax-regions/$REGION/class-rates/$BOOKS"
contains "no class rates left" '"class_rates":[]'
step "get taxed cart" 200 - "$BASE/storefront/carts/$TAX_CART"
contains "book pays the 25% default (1800 x 25/125)" '"tax_total":{"amount":360,"currency":"EUR"}'
step "lower the default to 13%" 200 - -X PATCH "$BASE/admin/tax-regions/$REGION" -H "$JSON" -d '{"default_rate_ppm":130000}'
step "get taxed cart" 200 - "$BASE/storefront/carts/$TAX_CART"
contains "book pays 13% (1800 x 13/113 = 207.1)" '"tax_total":{"amount":207,"currency":"EUR"}'
step "back to excluding tax" 200 - -X PATCH "$BASE/admin/markets/$MARKET" -H "$JSON" -d '{"prices_include_tax":false}'

echo "## inventory"
step "create lamp" 201 - -X POST "$BASE/admin/products" -H "$JSON" \
    -d '{"handle":"lamp","title":"Lamp","price":{"amount":1000,"currency":"EUR"}}'
LAMP=$(id)
LAMP_VAR=$(variant)
step "activate lamp" 200 - -X PATCH "$BASE/admin/products/$LAMP/status" -H "$JSON" -d '{"status":"active"}'
step "add lamp to catalog" 204 - -X POST "$BASE/admin/catalogs/$CATALOG/products" -H "$JSON" \
    -d "{\"products\":[\"$LAMP\"]}"
step "lamp has no stock set up" 200 - "$BASE/admin/variants/$LAMP_VAR/inventory"
contains "so it is untracked" '"id":null,'
contains "reported as tracked: false" '"tracked":false'
step "create warehouse for the market" 201 - -X POST "$BASE/admin/locations" -H "$JSON" \
    -d "{\"name\":\"Zagreb\",\"markets\":[\"$MARKET\"]}"
WAREHOUSE=$(id)
step "create lamp inventory" 201 - -X POST "$BASE/admin/inventory-items" -H "$JSON" \
    -d "{\"variant\":\"$LAMP_VAR\"}"
LAMP_ITEM=$(id)
contains "tracked by default" '"tracked":true,"continue_selling":false,"levels":[]'
step "2 lamps on hand" 200 - -X PUT "$BASE/admin/inventory-items/$LAMP_ITEM/levels/$WAREHOUSE" -H "$JSON" \
    -d '{"on_hand":2}'
contains "level is stored" "\"levels\":[{\"location\":\"$WAREHOUSE\",\"on_hand\":2,\"reserved\":0,\"available\":2}]"
step "create stock cart" 201 - -X POST "$BASE/storefront/carts" -H "$JSON" -d '{"country":"HR"}'
STOCK_CART=$(id)
step "add 3 lamps" 409 out_of_stock -X POST "$BASE/storefront/carts/$STOCK_CART/lines" -H "$JSON" \
    -d "{\"variant\":\"$LAMP_VAR\",\"quantity\":3}"
step "add 2 lamps" 200 - -X POST "$BASE/storefront/carts/$STOCK_CART/lines" -H "$JSON" \
    -d "{\"variant\":\"$LAMP_VAR\",\"quantity\":2}"
contains "lamps are available" '"fully_available":true'
step "count with a stale expected" 409 on_hand_changed -X PUT \
    "$BASE/admin/inventory-items/$LAMP_ITEM/levels/$WAREHOUSE" -H "$JSON" -d '{"on_hand":1,"expected_on_hand":5}'
step "a lamp broke (1 left)" 200 - -X PUT "$BASE/admin/inventory-items/$LAMP_ITEM/levels/$WAREHOUSE" \
    -H "$JSON" -d '{"on_hand":1,"expected_on_hand":2}'
step "get stock cart" 200 - "$BASE/storefront/carts/$STOCK_CART"
contains "lamp line ran out" '"status":"unavailable","reason_code":"out_of_stock"'
contains "and is left out of the total" '"total":{"amount":0,"currency":"EUR"},"fully_available":false'
step "delivery of 4" 200 - -X POST "$BASE/admin/inventory-items/$LAMP_ITEM/levels/$WAREHOUSE/adjustments" \
    -H "$JSON" -d '{"delta":4}'
contains "5 on hand" '"on_hand":5'
step "get stock cart" 200 - "$BASE/storefront/carts/$STOCK_CART"
contains "lamps are back" '"fully_available":true'
step "raise lamps beyond stock" 409 out_of_stock -X PUT "$BASE/storefront/carts/$STOCK_CART/lines/$LAMP_VAR" \
    -H "$JSON" -d '{"quantity":6}'
step "set lamps to 3" 200 - -X PUT "$BASE/storefront/carts/$STOCK_CART/lines/$LAMP_VAR" -H "$JSON" -d '{"quantity":3}'
contains "line holds 3" '"quantity":3'
step "stop tracking lamps" 200 - -X PATCH "$BASE/admin/inventory-items/$LAMP_ITEM" -H "$JSON" -d '{"tracked":false}'
step "add 100 untracked lamps" 200 - -X POST "$BASE/storefront/carts/$STOCK_CART/lines" -H "$JSON" \
    -d "{\"variant\":\"$LAMP_VAR\",\"quantity\":100}"
step "remove the lamp line" 200 - -X DELETE "$BASE/storefront/carts/$STOCK_CART/lines/$LAMP_VAR"
contains "cart is empty again" '"lines":[]'
step "remove it again" 404 line_not_found -X DELETE "$BASE/storefront/carts/$STOCK_CART/lines/$LAMP_VAR"
step "lower warehouse priority" 200 - -X PATCH "$BASE/admin/locations/$WAREHOUSE" -H "$JSON" -d '{"priority":5}'
contains "priority is stored" '"priority":5'
step "get warehouse" 200 - "$BASE/admin/locations/$WAREHOUSE"
step "list locations" 200 - "$BASE/admin/locations"
contains "the store started with a default one" '"name":"Default"'

echo "## customers"
step "create customer" 201 - -X POST "$BASE/admin/customers" -H "$JSON" \
    -d '{"email":" Ana@Example.HR ","first_name":"Ana"}'
CUSTOMER=$(id)
contains "email is normalized" '"email":"ana@example.hr","first_name":"Ana"'
step "rename customer" 200 - -X PATCH "$BASE/admin/customers/$CUSTOMER" -H "$JSON" \
    -d '{"first_name":null,"last_name":"Horvat"}'
contains "null clears, others change" '"first_name":null,"last_name":"Horvat"'
step "add shipping address" 201 - -X POST "$BASE/admin/customers/$CUSTOMER/addresses" -H "$JSON" \
    -d '{"last_name":"Horvat","line1":"Ilica 1","city":"Zagreb","postal_code":"10000","country":"HR","default_shipping":true}'
ADDRESS=$(address)
contains "it is the default shipping address" "\"default_shipping_address\":\"$ADDRESS\",\"default_billing_address\":null"
step "move it to Split, billing only" 200 - -X PUT "$BASE/admin/customers/$CUSTOMER/addresses/$ADDRESS" -H "$JSON" \
    -d '{"last_name":"Horvat","line1":"Riva 2","city":"Split","country":"HR","default_billing":true}'
contains "defaults follow the request" "\"default_shipping_address\":null,\"default_billing_address\":\"$ADDRESS\""
contains "address is replaced" '"city":"Split"'
step "get customer" 200 - "$BASE/admin/customers/$CUSTOMER"
step "create buyer cart" 201 - -X POST "$BASE/storefront/carts" -H "$JSON" -d '{"country":"HR"}'
BUYER_CART=$(id)
contains "no buyer yet" '"email":null,"customer":null'
step "set cart email" 200 - -X PUT "$BASE/storefront/carts/$BUYER_CART/email" -H "$JSON" -d '{"email":"Ivo@Example.hr"}'
contains "cart email is normalized" '"email":"ivo@example.hr","customer":null'
step "link the customer" 200 - -X PUT "$BASE/admin/carts/$BUYER_CART/customer" -H "$JSON" \
    -d "{\"customer\":\"$CUSTOMER\"}"
contains "cart takes the customer's email" "\"email\":\"ana@example.hr\",\"customer\":\"$CUSTOMER\""
step "unlink the customer" 200 - -X PUT "$BASE/admin/carts/$BUYER_CART/customer" -H "$JSON" -d '{"customer":null}'
contains "email stays" '"email":"ana@example.hr","customer":null'
step "remove the address" 200 - -X DELETE "$BASE/admin/customers/$CUSTOMER/addresses/$ADDRESS"
contains "address book is empty, defaults cleared" '"default_billing_address":null,"addresses":[]'

echo "## checkout with stock and tax"
# Books and lamps cost 900 (the price list takes 10%) and pay the region's 13% default;
# lamps are tracked again, 5 on hand.
step "track lamps again" 200 - -X PATCH "$BASE/admin/inventory-items/$LAMP_ITEM" -H "$JSON" -d '{"tracked":true}'
step "create shop cart" 201 - -X POST "$BASE/storefront/carts" -H "$JSON" -d '{"country":"HR"}'
SHOP_CART=$(id)
step "add 2 lamps" 200 - -X POST "$BASE/storefront/carts/$SHOP_CART/lines" -H "$JSON" \
    -d "{\"variant\":\"$LAMP_VAR\",\"quantity\":2}"
step "add a book" 200 - -X POST "$BASE/storefront/carts/$SHOP_CART/lines" -H "$JSON" \
    -d "{\"variant\":\"$BOOK_VAR\",\"quantity\":1}"
step "a guest's email" 200 - -X PUT "$BASE/storefront/carts/$SHOP_CART/email" -H "$JSON" \
    -d '{"email":"guest@example.hr"}'
step "ship to Zagreb" 200 - -X PUT "$BASE/storefront/carts/$SHOP_CART/shipping-address" -H "$JSON" \
    -d '{"address":{"last_name":"Horvat","line1":"Ilica 1","city":"Zagreb","country":"HR"}}'
step "standard (free)" 200 - -X PUT "$BASE/storefront/carts/$SHOP_CART/shipping/$(shgrp)" -H "$JSON" \
    -d "{\"option\":\"$STANDARD\"}"
step "checkout" 201 - -X POST "$BASE/storefront/carts/$SHOP_CART/checkout" -H "$JSON" -d '{}'
SHOP_ORDER=$(id)
GUEST=$(customer)
contains "the order copies the tax line" '"tax":{"rate_ppm":130000,"amount":{"amount":117,"currency":"EUR"},"tax_class":null}'
contains "and adds tax on top" '"subtotal":{"amount":2700,"currency":"EUR"},"shipping_total":{"amount":0,"currency":"EUR"},"tax_total":{"amount":351,"currency":"EUR"},"total":{"amount":3051'
step "the guest became a customer" 200 - "$BASE/admin/customers/$GUEST"
contains "with the cart's email" '"email":"guest@example.hr"'
step "the lamps are reserved" 200 - "$BASE/admin/inventory-items/$LAMP_ITEM"
contains "2 of 5" "\"on_hand\":5,\"reserved\":2,\"available\":3"
step "the checked-out cart" 200 - "$BASE/storefront/carts/$SHOP_CART"
contains "reads as charged, not out of stock" '"total":{"amount":3051,"currency":"EUR"},"fully_available":true'
step "a second cart, same guest" 201 - -X POST "$BASE/storefront/carts" -H "$JSON" -d '{"country":"HR"}'
AGAIN_CART=$(id)
step "add 3 lamps" 200 - -X POST "$BASE/storefront/carts/$AGAIN_CART/lines" -H "$JSON" \
    -d "{\"variant\":\"$LAMP_VAR\",\"quantity\":3}"
step "same email" 200 - -X PUT "$BASE/storefront/carts/$AGAIN_CART/email" -H "$JSON" \
    -d '{"email":"guest@example.hr"}'
step "ship to Zagreb" 200 - -X PUT "$BASE/storefront/carts/$AGAIN_CART/shipping-address" -H "$JSON" \
    -d '{"address":{"last_name":"Horvat","line1":"Ilica 1","city":"Zagreb","country":"HR"}}'
step "standard (free)" 200 - -X PUT "$BASE/storefront/carts/$AGAIN_CART/shipping/$(shgrp)" -H "$JSON" \
    -d "{\"option\":\"$STANDARD\"}"
step "a lamp broke (4 on hand, 2 reserved)" 200 - -X PUT \
    "$BASE/admin/inventory-items/$LAMP_ITEM/levels/$WAREHOUSE" -H "$JSON" -d '{"on_hand":4,"expected_on_hand":5}'
step "checkout with too few lamps" 409 cart_unavailable -X POST "$BASE/storefront/carts/$AGAIN_CART/checkout" \
    -H "$JSON" -d '{}'
contains "the error lists the lamp" "\"details\":{\"lines\":[\"$LAMP_VAR\"]}"
step "take 2 lamps instead" 200 - -X PUT "$BASE/storefront/carts/$AGAIN_CART/lines/$LAMP_VAR" -H "$JSON" \
    -d '{"quantity":2}'
step "checkout" 201 - -X POST "$BASE/storefront/carts/$AGAIN_CART/checkout" -H "$JSON" -d '{}'
contains "the same guest customer" "\"customer\":\"$GUEST\""
step "every lamp is reserved" 200 - "$BASE/admin/inventory-items/$LAMP_ITEM"
contains "4 of 4" '"on_hand":4,"reserved":4,"available":0'
step "the first order is unchanged" 200 - "$BASE/admin/orders/$SHOP_ORDER"

echo "## search (design note 0019)"
# Events mark what changed; the search sync job rebuilds it every 2 seconds.
wait_for "search sees every lamp reserved" "$BASE/storefront/search?country=HR&q=LAMP" '"available":false'
contains "priced in the market (the list takes 10%)" '"price":{"amount":900,"currency":"EUR"}'
contains "with facets" '"facets":['
step "the shop's listing (no q)" 200 - "$BASE/storefront/search?country=HR&sort=price_asc&limit=1"
contains "pages with a cursor" '"has_more":true,"next_cursor":"'
step "a filter is key:value" 422 invalid_filter "$BASE/storefront/search?country=HR&filter=Red"
step "an unknown facet" 422 unknown_facet "$BASE/storefront/search?country=HR&filter=colour:Red"
step "no market sells to Japan" 422 no_market_for_country "$BASE/storefront/search?country=JP"
step "too deep" 422 search_too_deep "$BASE/storefront/search?country=HR&limit=100&cursor=$(printf '{"v":1,"after":"950","filters":[["country","HR"]]}' | base64 | tr '+/' '-_' | tr -d '=\n')"
step "search settings" 200 - "$BASE/admin/search/settings"
contains "the defaults" '{"split_by":[],"facets":[]}'
step "split listings by colour" 200 - -X PATCH "$BASE/admin/search/settings" -H "$JSON" \
    -d '{"split_by":["Color"],"facets":[{"key":"vendor","label":"Brand"},{"key":"option.color"}]}'
step "at most 3 splits" 422 too_many_splits -X PATCH "$BASE/admin/search/settings" -H "$JSON" \
    -d '{"split_by":["a","b","c","d"]}'
step "back to one listing per product" 200 - -X PATCH "$BASE/admin/search/settings" -H "$JSON" \
    -d '{"split_by":[],"facets":[]}'
step "rebuild everything" 202 - -X POST "$BASE/admin/search/reindex"
wait_for "the rebuild finishes" "$BASE/admin/search/status" '"queued":0'

echo "## fulfilment (design note 0018)"
# The shop order: 2 lamps, reserved at the warehouse (4 on hand, 4 reserved with the
# second order's), and a book.
step "the shop order" 200 - "$BASE/admin/orders/$SHOP_ORDER"
LAMP_LINE=$(ordl 1)
contains "its lamps wait at the warehouse" "{\"line\":\"$LAMP_LINE\",\"location\":\"$WAREHOUSE\",\"quantity\":2}"
FULFILS="$BASE/admin/orders/$SHOP_ORDER/fulfillments"
ONE_LAMP="{\"location\":\"$WAREHOUSE\",\"lines\":[{\"line\":\"$LAMP_LINE\",\"quantity\":1}],\"tracking\":[{\"carrier\":\"DHL\",\"number\":\"JD1\",\"url\":\"https://dhl.example/JD1\"}],\"notify\":true}"
step "fulfil 3 of 2 lamps" 409 over_fulfilled -X POST "$FULFILS" -H "$JSON" \
    -d "{\"location\":\"$WAREHOUSE\",\"lines\":[{\"line\":\"$LAMP_LINE\",\"quantity\":3}]}"
contains "what's left comes back" "\"details\":{\"line\":\"$LAMP_LINE\",\"requested\":3,\"unfulfilled\":2}"
step "fulfil from an unknown location" 422 location_not_found -X POST "$FULFILS" -H "$JSON" \
    -d "{\"location\":\"loc_00000000000000000000000000000063\",\"lines\":[{\"line\":\"$LAMP_LINE\",\"quantity\":1}]}"
step "fulfil a lamp" 201 - -X POST "$FULFILS" -H "$JSON" -H 'idempotency-key: ship-1' -d "$ONE_LAMP"
FULFILLMENT=$(id)
contains "it shipped" '"status":"shipped"'
replayed "a retried fulfilment replays" -X POST "$FULFILS" -H "$JSON" -H 'idempotency-key: ship-1' -d "$ONE_LAMP"
step "the lamp left the warehouse" 200 - "$BASE/admin/inventory-items/$LAMP_ITEM"
contains "3 on hand, 3 reserved" '"on_hand":3,"reserved":3,"available":0'
step "the order is partly fulfilled" 200 - "$BASE/admin/orders/$SHOP_ORDER"
contains "one lamp left" '"fulfillment_status":"partially_fulfilled"'
step "new tracking" 200 - -X PATCH "$BASE/admin/fulfillments/$FULFILLMENT" -H "$JSON" \
    -d '{"tracking":[{"number":"JD2"}],"notify":true}'
contains "replaces the old" '"tracking":[{"carrier":null,"number":"JD2","url":null}]'
step "null tracking" 422 invalid_body -X PATCH "$BASE/admin/fulfillments/$FULFILLMENT" -H "$JSON" \
    -d '{"tracking":null}'
step "list the order's fulfilments" 200 - "$FULFILS"
contains "one" "\"items\":[{\"id\":\"$FULFILLMENT\""
step "cancel the order while it's partly shipped" 409 order_has_fulfillments -X POST \
    "$BASE/admin/orders/$SHOP_ORDER/cancel" -H "$JSON" -d '{"reason":"customer"}'
contains "naming the fulfilment" "\"details\":{\"fulfillments\":[\"$FULFILLMENT\"]}"
step "take the fulfilment back" 200 - -X POST "$BASE/admin/fulfillments/$FULFILLMENT/cancel"
contains "it's cancelled" '"status":"cancelled"'
step "the lamp is back" 200 - "$BASE/admin/inventory-items/$LAMP_ITEM"
contains "4 on hand, 4 reserved" '"on_hand":4,"reserved":4,"available":0'
step "a cancelled fulfilment's tracking" 409 fulfillment_cancelled -X PATCH \
    "$BASE/admin/fulfillments/$FULFILLMENT" -H "$JSON" -d '{"tracking":[]}'
step "cancel the order" 200 - -X POST "$BASE/admin/orders/$SHOP_ORDER/cancel" -H "$JSON" \
    -d '{"reason":"customer","notify":true}'
contains "with its reason" '"cancel_reason":"customer"'
contains "nothing left to ship" '"unfulfilled":[]'
step "its lamps are released" 200 - "$BASE/admin/inventory-items/$LAMP_ITEM"
contains "4 on hand, 2 reserved" '"on_hand":4,"reserved":2,"available":2'
step "fulfil a cancelled order" 409 order_cancelled -X POST "$FULFILS" -H "$JSON" -d "$ONE_LAMP"
step "mark a cancelled order paid" 409 order_cancelled -X POST "$BASE/admin/orders/$SHOP_ORDER/paid"
step "an unknown cancel reason" 422 invalid_body -X POST "$BASE/admin/orders/$SHOP_ORDER/cancel" -H "$JSON" \
    -d '{"reason":"bored"}'
step "unknown fulfilment" 404 fulfillment_not_found "$BASE/admin/fulfillments/ful_00000000000000000000000000000063"
step "fulfilments of an unknown order" 404 order_not_found \
    "$BASE/admin/orders/ord_00000000000000000000000000000063/fulfillments"

step "stop tracking lamps again" 200 - -X PATCH "$BASE/admin/inventory-items/$LAMP_ITEM" -H "$JSON" \
    -d '{"tracked":false}'

echo "## apps and webhooks (design note 0016)"
# A webhook receiver when python3 is here (it is in CI): the host must run with
# HYPHA_ALLOW_PRIVATE_CALLBACKS=true to reach it. Without one, the delivery fails and the
# script checks that instead.
HOOK_PORT=${E2E_HOOK_PORT:-3199}
HOOKS_FILE=$(mktemp)
HOOKS=''
if command -v python3 >/dev/null 2>&1; then
    python3 - "$HOOK_PORT" "$HOOKS_FILE" <<'PY' &
import http.server, sys

port, out = int(sys.argv[1]), sys.argv[2]

class Hook(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        body = self.rfile.read(int(self.headers.get("content-length", 0)))
        with open(out, "ab") as f:
            f.write(self.headers.get("hypha-event-type", "").encode() + b" " + body + b"\n")
        self.send_response(204)
        self.end_headers()

    def log_message(self, *args):
        pass

http.server.HTTPServer(("127.0.0.1", port), Hook).serve_forever()
PY
    HOOKS=$!
    # Not a job of this shell any more, so killing it at the end prints nothing.
    disown "$HOOKS"
    trap 'kill $HOOKS 2>/dev/null; rm -f "$HOOKS_FILE"' EXIT
    sleep 1
fi
HOOK_URL="http://127.0.0.1:$HOOK_PORT/hooks"
step "an app with a webhook" 201 - -X POST "$BASE/admin/apps" -H "$JSON" \
    -d "{\"key\":\"app_erp\",\"name\":\"ERP\",\"subscriptions\":[\"product.created\"],\"webhook\":{\"url\":\"$HOOK_URL\",\"timeout_ms\":2000}}"
contains "its secret is shown once" '"secret":"hypha_as_'
step "the same key again" 409 app_exists -X POST "$BASE/admin/apps" -H "$JSON" -d '{"key":"app_erp","name":"Again"}'
step "a key without app_" 422 invalid_app_key -X POST "$BASE/admin/apps" -H "$JSON" -d '{"key":"erp","name":"ERP"}'
step "an event type that can't be" 422 invalid_subscription -X POST "$BASE/admin/apps" -H "$JSON" \
    -d '{"key":"app_bad","name":"Bad","subscriptions":["Orders"]}'
step "a webhook timeout out of range" 422 invalid_timeout -X POST "$BASE/admin/apps" -H "$JSON" \
    -d '{"key":"app_bad","name":"Bad","webhook":{"url":"https://bad.example.com","timeout_ms":50}}'
step "list apps" 200 - "$BASE/admin/apps"
contains "the courier from shipping is one" '"key":"app_courier","name":"Courier","enabled":true'
step "get the app" 200 - "$BASE/admin/apps/app_erp"
contains "with its webhook, without its secret" "\"webhook\":{\"url\":\"$HOOK_URL\",\"timeout_ms\":2000}"
step "an unknown app" 404 app_not_found "$BASE/admin/apps/app_none"
step "a product the app hears about" 201 - -X POST "$BASE/admin/products" -H "$JSON" \
    -d '{"handle":"webhook-mug","title":"Webhook mug","price":{"amount":1000,"currency":"EUR"}}'
if [[ -n $HOOKS ]]; then
    wait_for "the dispatcher delivers it" "$BASE/admin/apps/app_erp/deliveries?status=succeeded" '"event_type":"product.created"'
    if grep -q '"handle":"webhook-mug"' "$HOOKS_FILE"; then
        echo "ok   the receiver got the envelope"
    else
        echo "FAIL the receiver got the envelope: $(cat "$HOOKS_FILE")"
        FAILED=$((FAILED + 1))
    fi
else
    wait_for "the delivery fails, to be retried" "$BASE/admin/apps/app_erp/deliveries?status=pending" '"attempts":1'
fi
DELIVERY=$(sed -E 's/.*"id":"(dlv_[0-9a-f]+)".*/\1/' <<<"$BODY")
EVENT=$(sed -E 's/.*"event_id":"(evt_[0-9a-f]+)".*/\1/' <<<"$BODY")
step "retry it" 200 - -X POST "$BASE/admin/apps/app_erp/deliveries/$DELIVERY/retry"
contains "due again" '"status":"pending"'
step "deliveries of a status that isn't one" 400 invalid_query "$BASE/admin/apps/app_erp/deliveries?status=lost"
step "retry a delivery that isn't the app's" 404 delivery_not_found \
    -X POST "$BASE/admin/apps/app_erp/deliveries/dlv_00000000000000000000000000000000/retry"
step "the event as sent" 200 - "$BASE/admin/events/$EVENT"
contains "its envelope" '"type":"product.created","version":1,"subject":{"type":"product"'
step "an unknown event" 404 event_not_found "$BASE/admin/events/evt_00000000000000000000000000000000"
step "remove the webhook" 200 - -X PATCH "$BASE/admin/apps/app_erp" -H "$JSON" -d '{"webhook":null}'
contains "it has none" '"webhook":null'
step "a new secret" 200 - -X POST "$BASE/admin/apps/app_erp/secret"
contains "shown once" '"secret":"hypha_as_'
step "delete the app" 204 - -X DELETE "$BASE/admin/apps/app_erp"
step "it's gone" 404 app_not_found "$BASE/admin/apps/app_erp"

echo
echo "## errors"
step "unknown route" 404 route_not_found "$BASE/nope"
step "wrong method" 405 method_not_allowed -X DELETE "$BASE/admin/products/$MUG"
step "malformed json" 400 malformed_json -X POST "$BASE/admin/markets" -H "$JSON" -d '{"name":'
step "missing field" 422 invalid_body -X POST "$BASE/admin/markets" -H "$JSON" -d '{"name":"X"}'
step "unknown currency" 422 invalid_body -X POST "$BASE/admin/markets" -H "$JSON" \
    -d '{"name":"UK","countries":["GB"],"currency":"GBP"}'
step "amount beyond 2^53-1" 422 invalid_body -X POST "$BASE/admin/products" -H "$JSON" \
    -d '{"handle":"huge","title":"Huge","price":{"amount":9007199254740992,"currency":"EUR"}}'
step "wrong id prefix" 400 invalid_path "$BASE/storefront/carts/$MUG"
step "unknown cart" 404 cart_not_found "$BASE/storefront/carts/cart_00000000000000000000000000000063"
step "duplicate handle" 409 handle_taken -X POST "$BASE/admin/products" -H "$JSON" \
    -d '{"handle":"mug","title":"Mug 2","price":{"amount":1,"currency":"EUR"}}'
step "country in other market" 409 country_in_other_market -X POST "$BASE/admin/markets" -H "$JSON" \
    -d '{"name":"DACH","countries":["AT","DE"],"currency":"EUR"}'
step "create Serbia market" 201 - -X POST "$BASE/admin/markets" -H "$JSON" \
    -d '{"name":"Serbia","countries":["RS"],"currency":"RSD"}'
SERBIA=$(id)
step "add a country another market has" 409 country_in_other_market -X POST \
    "$BASE/admin/markets/$MARKET/countries" -H "$JSON" -d '{"country":"RS"}'
step "remove a market's last country" 422 last_country -X DELETE "$BASE/admin/markets/$SERBIA/countries/RS"
step "remove a country the market lacks" 404 country_not_in_market -X DELETE \
    "$BASE/admin/markets/$MARKET/countries/FR"
step "second catalog for market" 409 catalog_exists -X POST "$BASE/admin/catalogs" -H "$JSON" \
    -d "{\"market\":\"$MARKET\"}"
step "create RSD price list" 201 - -X POST "$BASE/admin/price-lists" -H "$JSON" -d '{"currency":"RSD"}'
RSD_LIST=$(id)
step "assign RSD list to EUR market" 422 price_list_currency_mismatch -X PUT \
    "$BASE/admin/catalogs/$CATALOG/price-list" -H "$JSON" -d "{\"price_list\":\"$RSD_LIST\"}"
step "fixed prices on unknown list" 404 price_list_not_found -X PUT \
    "$BASE/admin/price-lists/pl_00000000000000000000000000000063/fixed-prices" -H "$JSON" -d '{"fixed_prices":[]}'
step "fixed price in other currency" 422 fixed_price_currency_mismatch -X PUT \
    "$BASE/admin/price-lists/$LIST/fixed-prices" -H "$JSON" \
    -d "{\"fixed_prices\":[{\"variant\":\"$CUP_VAR\",\"price\":{\"amount\":200,\"currency\":\"RSD\"}}]}"
step "misspelled PATCH field" 422 invalid_body -X PATCH "$BASE/admin/customers/$CUSTOMER" -H "$JSON" \
    -d '{"frist_name":"Ana"}'
step "null for a field that can't be cleared" 422 invalid_body -X PATCH \
    "$BASE/admin/customers/$CUSTOMER" -H "$JSON" -d '{"email":null}'
step "empty PATCH changes nothing" 200 - -X PATCH "$BASE/admin/markets/$MARKET" -H "$JSON" -d '{}'
step "update unknown market" 404 market_not_found -X PATCH \
    "$BASE/admin/markets/mkt_00000000000000000000000000000063" -H "$JSON" -d '{"prices_include_tax":true}'
step "products for unknown catalog" 404 catalog_not_found -X POST \
    "$BASE/admin/catalogs/cat_00000000000000000000000000000063/products" -H "$JSON" -d '{"products":[]}'
step "add archived mug" 422 product_not_active -X POST "$BASE/storefront/carts/$CART/lines" -H "$JSON" \
    -d "{\"variant\":\"$MUG_VAR\",\"quantity\":1}"
step "cart to a country without market" 422 no_market_for_country -X POST "$BASE/storefront/carts" -H "$JSON" \
    -d '{"country":"US"}'
step "cart to a lowercase country" 422 invalid_country_code -X POST "$BASE/storefront/carts" -H "$JSON" \
    -d '{"country":"us"}'
step "second tax region for a country" 409 tax_region_exists -X POST "$BASE/admin/tax-regions" -H "$JSON" \
    -d '{"country":"HR","default_rate_ppm":250000}'
step "tax rate above 100%" 422 invalid_tax_rate -X POST "$BASE/admin/tax-regions" -H "$JSON" \
    -d '{"country":"DE","default_rate_ppm":1000001}'
step "unknown tax region" 404 tax_region_not_found "$BASE/admin/tax-regions/txreg_00000000000000000000000000000063"
step "rate for unknown class" 404 tax_class_not_found -X PUT \
    "$BASE/admin/tax-regions/$REGION/class-rates/txc_00000000000000000000000000000063" -H "$JSON" -d '{"rate_ppm":0}'
step "remove a rate the class doesn't have" 404 class_rate_not_found -X DELETE \
    "$BASE/admin/tax-regions/$REGION/class-rates/$BOOKS"
step "assign unknown class" 422 tax_class_not_found -X PUT "$BASE/admin/tax-assignments" -H "$JSON" \
    -d "{\"products\":[\"$BOOK\"],\"tax_class\":\"txc_00000000000000000000000000000063\"}"
step "second item for a variant" 409 inventory_item_exists -X POST "$BASE/admin/inventory-items" -H "$JSON" \
    -d "{\"variant\":\"$LAMP_VAR\"}"
step "unknown inventory item" 404 inventory_item_not_found \
    "$BASE/admin/inventory-items/invi_00000000000000000000000000000063"
step "stock at unknown location" 404 location_not_found -X PUT \
    "$BASE/admin/inventory-items/$LAMP_ITEM/levels/loc_00000000000000000000000000000063" -H "$JSON" -d '{"on_hand":1}'
step "unknown location" 404 location_not_found "$BASE/admin/locations/loc_00000000000000000000000000000063"
step "location listing a market twice" 422 duplicate_market -X POST "$BASE/admin/locations" -H "$JSON" \
    -d "{\"name\":\"Split\",\"markets\":[\"$MARKET\",\"$MARKET\"]}"
step "blank location name" 422 field_required -X POST "$BASE/admin/locations" -H "$JSON" -d '{"name":" "}'
step "stock beyond 2^53-1" 422 stock_out_of_range -X PUT \
    "$BASE/admin/inventory-items/$LAMP_ITEM/levels/$WAREHOUSE" -H "$JSON" -d '{"on_hand":9007199254740992}'
step "same email in another case" 409 customer_exists -X POST "$BASE/admin/customers" -H "$JSON" \
    -d '{"email":"ANA@example.hr"}'
step "invalid customer email" 422 invalid_email -X POST "$BASE/admin/customers" -H "$JSON" -d '{"email":"ana"}'
step "address without a city" 422 invalid_address -X POST "$BASE/admin/customers/$CUSTOMER/addresses" \
    -H "$JSON" -d '{"last_name":"Horvat","line1":"Ilica 1","city":" ","country":"HR"}'
step "unknown customer" 404 customer_not_found "$BASE/admin/customers/cus_00000000000000000000000000000063"
step "remove unknown address" 404 address_not_found -X DELETE \
    "$BASE/admin/customers/$CUSTOMER/addresses/addr_00000000000000000000000000000063"
step "link unknown customer to cart" 422 customer_not_found -X PUT "$BASE/admin/carts/$BUYER_CART/customer" \
    -H "$JSON" -d '{"customer":"cus_00000000000000000000000000000063"}'
step "invalid cart email" 422 invalid_email -X PUT "$BASE/storefront/carts/$BUYER_CART/email" -H "$JSON" \
    -d '{"email":"nope"}'
step "zero quantity" 422 invalid_quantity -X POST "$BASE/storefront/carts/$CART/lines" -H "$JSON" \
    -d "{\"variant\":\"$CUP_VAR\",\"quantity\":0}"
step "get mug" 200 - "$BASE/admin/products/$MUG"
contains "mug has its handle" '"handle":"mug"'
step "unknown product" 404 product_not_found "$BASE/admin/products/prod_00000000000000000000000000000063"
step "get lamp inventory item" 200 - "$BASE/admin/inventory-items/$LAMP_ITEM"
step "handle with a space" 422 invalid_handle -X POST "$BASE/admin/products" -H "$JSON" \
    -d '{"handle":"blue mug","title":"Mug","price":{"amount":1,"currency":"EUR"}}'
step "blank product title" 422 field_required -X POST "$BASE/admin/products" -H "$JSON" \
    -d '{"handle":"untitled","title":" ","price":{"amount":1,"currency":"EUR"}}'
step "body without content type" 415 missing_content_type -X POST "$BASE/admin/products" \
    -d '{"handle":"x","title":"X","price":{"amount":1,"currency":"EUR"}}'
step "market without countries" 422 no_countries -X POST "$BASE/admin/markets" -H "$JSON" \
    -d '{"name":"Nowhere","countries":[],"currency":"EUR"}'
step "market listing a country twice" 422 duplicate_country -X POST "$BASE/admin/markets" -H "$JSON" \
    -d '{"name":"France","countries":["FR","FR"],"currency":"EUR"}'
step "discount beyond 100%" 422 invalid_basis_points -X POST "$BASE/admin/price-lists" -H "$JSON" \
    -d '{"currency":"EUR","adjustment_bps":-10001}'
step "the same variant twice in fixed prices" 422 duplicate_fixed_price -X PUT \
    "$BASE/admin/price-lists/$LIST/fixed-prices" -H "$JSON" \
    -d "{\"fixed_prices\":[{\"variant\":\"$CUP_VAR\",\"price\":{\"amount\":1,\"currency\":\"EUR\"}},{\"variant\":\"$CUP_VAR\",\"price\":{\"amount\":2,\"currency\":\"EUR\"}}]}"
step "unknown price list in a body" 422 price_list_not_found -X PUT "$BASE/admin/catalogs/$CATALOG/price-list" \
    -H "$JSON" -d '{"price_list":"pl_00000000000000000000000000000063"}'
step "unknown market in a body" 422 market_not_found -X POST "$BASE/admin/catalogs" -H "$JSON" \
    -d '{"market":"mkt_00000000000000000000000000000063"}'
step "unknown variant in a cart line" 422 variant_not_found -X POST "$BASE/storefront/carts/$CART/lines" \
    -H "$JSON" -d '{"variant":"var_00000000000000000000000000000063","quantity":1}'
step "create overflow cart" 201 - -X POST "$BASE/storefront/carts" -H "$JSON" -d '{"country":"HR"}'
OVERFLOW_CART=$(id)
step "add 4294967295 lamps" 200 - -X POST "$BASE/storefront/carts/$OVERFLOW_CART/lines" -H "$JSON" \
    -d "{\"variant\":\"$LAMP_VAR\",\"quantity\":4294967295}"
step "one lamp too many" 422 quantity_overflow -X POST "$BASE/storefront/carts/$OVERFLOW_CART/lines" -H "$JSON" \
    -d "{\"variant\":\"$LAMP_VAR\",\"quantity\":1}"
LONG=$(printf 'x%.0s' $(seq 1 256))
step "first name over 255 characters" 422 field_too_long -X PATCH "$BASE/admin/customers/$CUSTOMER" \
    -H "$JSON" -d "{\"first_name\":\"$LONG\"}"
step "create draft product" 201 - -X POST "$BASE/admin/products" -H "$JSON" \
    -d '{"handle":"draft","title":"Draft","price":{"amount":1,"currency":"EUR"}}'
step "draft -> archived" 422 invalid_status_transition -X PATCH "$BASE/admin/products/$(id)/status" \
    -H "$JSON" -d '{"status":"archived"}'

echo
echo "## auth (design note 0008)"
step "who am I" 200 - "$BASE/admin/me"
contains "the owner has the owner role" '"name":"Owner"'
OWNER=$(sed -E 's/.*"user":\{"id":"(user_[0-9a-f]+)".*/\1/' <<<"$BODY")
step "the last owner can't be disabled" 409 last_owner -X PATCH "$BASE/admin/users/$OWNER" \
    -H "$JSON" -d '{"status":"disabled"}'
step "create a read-only key" 201 - -X POST "$BASE/admin/api-keys" -H "$JSON" \
    -d '{"kind":"secret","name":"Reader","scopes":["products:read"]}'
contains "the key is shown redacted" '"redacted":"hypha_sk_…'
READER=$(token)
READER_ID=$(id)
SAVED_AUTH=("${AUTH[@]}")
AUTH=(-H "authorization: Bearer $READER")
step "the key reads products" 200 - "$BASE/admin/products/$MUG"
step "but can't write them" 403 missing_scope -X POST "$BASE/admin/products" -H "$JSON" \
    -d '{"handle":"nope","title":"Nope","price":{"amount":1,"currency":"EUR"}}'
step "a key is no session" 401 session_required "$BASE/admin/me"
AUTH=("${SAVED_AUTH[@]}")
step "revoke the key" 200 - -X POST "$BASE/admin/api-keys/$READER_ID/revoke"
step "revoking twice" 409 api_key_revoked -X POST "$BASE/admin/api-keys/$READER_ID/revoke"
AUTH=(-H "authorization: Bearer $READER")
step "a revoked key stops working" 401 invalid_token "$BASE/admin/products/$MUG"
AUTH=("${SAVED_AUTH[@]}")
step "a publishable key's scopes are fixed" 422 api_key_scopes_fixed -X POST \
    "$BASE/admin/api-keys" -H "$JSON" -d '{"kind":"publishable","name":"Shop","scopes":["orders:read"]}'
step "create a publishable key" 201 - -X POST "$BASE/admin/api-keys" -H "$JSON" \
    -d '{"kind":"publishable","name":"Storefront"}'
PUBLISHABLE=$(token)
SAVED_SF_AUTH=("${SF_AUTH[@]}")
SF_AUTH=(-H "x-publishable-key: $PUBLISHABLE")
step "a storefront reads the store" 200 - "$BASE/storefront/store"
contains "only its name" '{"name":'
step "and makes a cart" 201 - -X POST "$BASE/storefront/carts" -H "$JSON" -d '{"country":"HR"}'
SF_AUTH=("${SAVED_SF_AUTH[@]}")
AUTH=(-H "x-publishable-key: $PUBLISHABLE")
step "but never uses admin routes" 403 admin_credentials_required -X POST \
    "$BASE/admin/customers" -H "$JSON" -d '{"email":"shop@example.com"}'
AUTH=("${SAVED_AUTH[@]}")
SF_AUTH=(-H "authorization: Bearer $SESSION")
step "and a staff session never uses the storefront" 403 publishable_key_required \
    "$BASE/storefront/store"
SF_AUTH=("${SAVED_SF_AUTH[@]}")
step "create a role" 201 - -X POST "$BASE/admin/roles" -H "$JSON" \
    -d '{"name":"Support","scopes":["orders:write","customers:read"]}'
ROLE=$(id)
step "role names are unique ignoring case" 409 role_name_taken -X POST "$BASE/admin/roles" -H "$JSON" \
    -d '{"name":"SUPPORT","scopes":["orders:read"]}'
step "the owner role can't change" 409 owner_role_fixed -X PATCH \
    "$BASE/admin/roles/$(sed -E 's/.*"role":"(role_[0-9a-f]+)".*/\1/' <<<"$(curl -s "${AUTH[@]}" "$BASE/admin/users/$OWNER")")" \
    -H "$JSON" -d '{"name":"Boss"}'
step "invite someone" 201 - -X POST "$BASE/admin/invites" -H "$JSON" \
    -d "{\"email\":\"support@example.com\",\"role\":\"$ROLE\"}"
step "staff can't be invited" 409 user_exists -X POST "$BASE/admin/invites" -H "$JSON" \
    -d "{\"email\":\"$OWNER_EMAIL\",\"role\":\"$ROLE\"}"
AUTH=()
step "a made-up invite token" 401 invalid_token -X POST "$BASE/admin/invites/accept" -H "$JSON" \
    -d '{"token":"hypha_inv_nope","name":"Eve","password":"eve password"}'
step "a wrong password" 401 invalid_credentials -X POST "$BASE/admin/auth/login" -H "$JSON" \
    -d "{\"email\":\"$OWNER_EMAIL\",\"password\":\"wrong password\"}"
step "a reset for anyone is 202" 202 - -X POST "$BASE/admin/auth/password-reset" -H "$JSON" \
    -d '{"email":"nobody@example.com"}'
step "a made-up reset token" 401 invalid_token -X POST "$BASE/admin/auth/password-reset/confirm" \
    -H "$JSON" -d '{"token":"hypha_rst_nope","password":"new password"}'
AUTH=(-H "authorization: Bearer $SESSION")
step "log out" 204 - -X POST "$BASE/admin/auth/logout"
step "the session is over" 401 invalid_token "$BASE/admin/me"

echo
echo "## rate limits (design note 0009)"
# Last on purpose: this uses up the login allowance of the address the script runs from.
# Ten more tries on top of the script's earlier logins: well past both the address's and
# the email-and-address limits, whichever trips first.
AUTH=()
for n in $(seq 1 10); do
    curl -s -o /dev/null -X POST "$BASE/admin/auth/login" -H "$JSON" \
        -d '{"email":"guess@example.com","password":"guess"}'
done
step "a run of login tries from one address is refused" 429 rate_limited -X POST "$BASE/admin/auth/login" -H "$JSON" \
    -d '{"email":"guess@example.com","password":"guess"}'
contains "and says when to come back" '"retry_after_secs":'

echo
if ((FAILED > 0)); then
    echo "$FAILED step(s) failed"
    exit 1
fi
echo "all steps passed"
