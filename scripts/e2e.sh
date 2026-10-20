#!/usr/bin/env bash
# End-to-end check against a running hypha-host-axum on a fresh database, with file
# storage (`docker compose up -d`; see CLAUDE.md).
#
#   HYPHA_OWNER_EMAIL=owner@example.com HYPHA_OWNER_PASSWORD=e2e-owner-password \
#   HYPHA_ALLOW_PRIVATE_CALLBACKS=true HYPHA_ADMIN_URL=http://admin.e2e.test \
#       cargo run -p hypha-host-axum  # in one terminal (PORT=... if 3000 is taken)
#   bash scripts/e2e.sh               # in another (Git Bash on Windows; HYPHA_URL=...)
#
# The script logs in as that owner (E2E_OWNER_EMAIL / E2E_OWNER_PASSWORD if you chose
# others) and sends the session with every step; the "auth" section checks the rest.
#
# Emails (design note 0028): with the host sending over SMTP (`cargo run` does, to the
# compose Mailpit) and Mailpit's API at MAILPIT_URL (default http://localhost:8025), the
# script checks that the order and invite emails arrived, then deletes them. With
# another sender (the log, as on Workers) or E2E_MAILPIT=0 it skips those checks.
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

# The email checks (design note 0028): on when the host sends over SMTP and Mailpit is
# reachable (decided after logging in), unless E2E_MAILPIT=0.
MAILPIT="${MAILPIT_URL:-http://localhost:8025}"
MAIL_CHECKS=0

# mailpit_query TO SUBJECT: Mailpit's search for messages to TO with SUBJECT (words).
mailpit_query() { echo "to:$1 subject:\"$2\""; }

# mail NAME TO SUBJECT: waits up to 20 s for an email to TO whose subject has SUBJECT
# in Mailpit, then deletes what matched. Skipped without the email checks.
mail() {
    local name=$1 query n found
    ((MAIL_CHECKS)) || return 0
    query=$(mailpit_query "$2" "$3")
    for n in $(seq 1 20); do
        found=$(curl -s -G "$MAILPIT/api/v1/search" --data-urlencode "query=$query")
        if [[ $found == *'"messages_count":'[1-9]* ]]; then
            echo "ok   $name"
            curl -s -o /dev/null -X DELETE -G "$MAILPIT/api/v1/search" --data-urlencode "query=$query"
            return
        fi
        sleep 1
    done
    echo "FAIL $name: no email to $2 with \"$3\" in Mailpit within 20 s: $found"
    FAILED=$((FAILED + 1))
}

# mail_token TO SUBJECT PREFIX: prints the first token starting with PREFIX (like
# hypha_inv_) in the latest email to TO whose subject has SUBJECT, waiting up to 20 s;
# nothing without the email checks. The email stays for `mail` to find.
mail_token() {
    local query n found message
    ((MAIL_CHECKS)) || return 0
    query=$(mailpit_query "$1" "$2")
    for n in $(seq 1 20); do
        found=$(curl -s -G "$MAILPIT/api/v1/search" --data-urlencode "query=$query")
        message=$(grep -o '"ID":"[^"]*"' <<<"$found" | head -1 | sed -E 's/"ID":"([^"]+)"/\1/')
        if [[ -n $message ]]; then
            curl -s "$MAILPIT/api/v1/message/$message" | grep -o "$3[0-9a-f]*" | head -1
            return
        fi
        sleep 1
    done
}

# forget_mail TO: deletes what an earlier run left in Mailpit for TO.
forget_mail() {
    ((MAIL_CHECKS)) || return 0
    curl -s -o /dev/null -X DELETE -G "$MAILPIT/api/v1/search" --data-urlencode "query=to:$1"
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
# The top-level id of the last payment method response.
pm() { sed -E 's/^[^"]*"id":"(pm_[0-9a-f]+)".*/\1/' <<<"$BODY"; }
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
step "the email settings (design note 0028)" 200 - "$BASE/admin/notification-settings"
SENDER=$(sed -E 's/.*"sender":"([a-z]+)".*/\1/' <<<"$BODY")
if [[ ${E2E_MAILPIT:-1} != 0 && $SENDER == smtp ]] && curl -sf -o /dev/null "$MAILPIT/api/v1/info"; then
    MAIL_CHECKS=1
    echo "     emails: checked in Mailpit at $MAILPIT"
else
    echo "     emails: not checked (sender $SENDER; E2E_MAILPIT=${E2E_MAILPIT:-1})"
fi
for to in guest@example.hr support@example.com "$OWNER_EMAIL"; do
    forget_mail "$to"
done
step "the Online Store channel (design note 0020)" 200 - "$BASE/admin/channels"
contains "every store starts with one" '"name":"Online Store","kind":"online_store"'
ONLINE_STORE=$(sed -E 's/.*"items":\[\{"id":"([^"]+)".*/\1/' <<<"$BODY")
step "a publishable key for the storefront steps" 201 - -X POST "$BASE/admin/api-keys" -H "$JSON" \
    -d "{\"kind\":\"publishable\",\"name\":\"e2e storefront\",\"channel\":\"$ONLINE_STORE\"}"
contains "sells through it" "\"channel\":\"$ONLINE_STORE\""
SF_AUTH=(-H "x-publishable-key: $(token)")
step "a publishable key needs a channel" 422 api_key_channel_required -X POST "$BASE/admin/api-keys" \
    -H "$JSON" -d '{"kind":"publishable","name":"Nowhere"}'

echo
echo "## happy path"
step "health" 200 - "$BASE/health"
step "the store this deployment serves" 200 - "$BASE/admin/store"
contains "store has a store_ ID" '"id":"store_'
step "rename the store" 200 - -X PATCH "$BASE/admin/store" -H "$JSON" -d '{"name":"E2E Shop"}'
contains "store has its new name" '"name":"E2E Shop"'
step "a blank store name" 422 store_name_empty -X PATCH "$BASE/admin/store" -H "$JSON" -d '{"name":" "}'
step "a new store writes in English" 200 - "$BASE/admin/store"
contains "default language en" '"default_language":"en"'
contains "the setup guide hasn't started" '"setup":{"step":null,"finished_at":null}'
step "remember the setup guide's step (design note 0024)" 200 - -X PATCH "$BASE/admin/store" -H "$JSON" -d '{"setup_step":"market"}'
contains "the step is kept" '"step":"market"'
step "a setup step that isn't one" 422 invalid_setup_step -X PATCH "$BASE/admin/store" -H "$JSON" -d '{"setup_step":"Market!"}'
step "store details" 200 - -X PATCH "$BASE/admin/store" -H "$JSON" \
    -d '{"contact_email":"Hello@E2E.example","description":"Mugs and cups.","timezone":"Europe/Zagreb","legal_address":{"last_name":"E2E","line1":"Ilica 1","city":"Zagreb","country":"HR"}}'
contains "the email is lowercased" '"contact_email":"hello@e2e.example"'
contains "the address is kept" '"city":"Zagreb"'
step "an unknown time zone" 422 unknown_timezone -X PATCH "$BASE/admin/store" -H "$JSON" -d '{"timezone":"Mars/Base"}'
step "a malformed language" 422 invalid_language -X PATCH "$BASE/admin/store" -H "$JSON" -d '{"default_language":"en_GB"}'
step "clear the legal address" 200 - -X PATCH "$BASE/admin/store" -H "$JSON" -d '{"legal_address":null}'
contains "it is gone" '"legal_address":null'
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
step "an unknown filter" 400 invalid_query "$BASE/admin/products?colour=red"
step "an unknown sort" 400 invalid_query "$BASE/admin/products?sort=title"
step "products oldest first (design note 0024)" 200 - "$BASE/admin/products?sort=oldest&limit=1"
contains "starts with the mug" "\"items\":[{\"id\":\"$MUG\""
NEXT=$(sed -E 's/.*"next_cursor":"([^"]+)".*/\1/' <<<"$BODY")
step "the next page of that sort" 200 - "$BASE/admin/products?sort=oldest&limit=1&cursor=$NEXT"
contains "is the cup" "\"id\":\"$CUP\""
step "that cursor under another sort" 400 invalid_cursor "$BASE/admin/products?sort=title_asc&limit=1&cursor=$NEXT"
step "products of any of two statuses, by title" 200 - "$BASE/admin/products?status=active&status=draft&sort=title_desc"
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
VARIANTS="{\"variants\":[{\"id\":\"$SHIRT_M\",\"sku\":\"SHIRT-M\",\"price\":{\"amount\":1600,\"currency\":\"EUR\"},\"selections\":[\"$SIZE_M\"],\"weight_grams\":null,\"requires_shipping\":true},{\"id\":\"$SHIRT_S\",\"sku\":\"SHIRT-S\",\"price\":{\"amount\":1400,\"currency\":\"EUR\"},\"selections\":[\"$SIZE_S\"],\"weight_grams\":250,\"requires_shipping\":true}]}"
step "the whole variant list in one PUT (0021 Q1)" 200 - -X PUT "$BASE/admin/products/$SHIRT/variants" \
    -H "$JSON" -d "$VARIANTS"
contains "M first now, S weighed" '"weight_grams":250'
step "a list leaving a key out" 422 invalid_body -X PUT "$BASE/admin/products/$SHIRT/variants" -H "$JSON" \
    -d "{\"variants\":[{\"id\":\"$SHIRT_S\",\"price\":{\"amount\":1400,\"currency\":\"EUR\"},\"selections\":[\"$SIZE_S\"],\"weight_grams\":null,\"requires_shipping\":true}]}"
step "an empty list" 409 no_variants -X PUT "$BASE/admin/products/$SHIRT/variants" -H "$JSON" -d '{"variants":[]}'
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
step "the library by name (design note 0024)" 200 - "$BASE/admin/assets?sort=name_asc&q=png"
step "an unknown library sort" 400 invalid_query "$BASE/admin/assets?sort=size"
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

step "currencies for pickers" 200 - "$BASE/admin/currencies"
contains "yen has no minor units" '{"code":"JPY","name":"Japanese Yen","minor_units":0}'
contains "dinar has three" '{"code":"KWD","name":"Kuwaiti Dinar","minor_units":3}'
step "market in a metal" 422 invalid_body -X POST "$BASE/admin/markets" -H "$JSON" \
    -d '{"name":"Gold","countries":["CH"],"currency":"XAU"}'
step "market to an unassigned country" 422 unknown_country_code -X POST "$BASE/admin/markets" -H "$JSON" \
    -d '{"name":"Nowhere","countries":["ZZ"],"currency":"EUR"}'
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
step "list markets" 200 - "$BASE/admin/markets"
contains "a market names its catalog" "\"catalog\":\"$CATALOG\""
step "fetch the market" 200 - "$BASE/admin/markets/$MARKET"
step "an unknown market" 404 market_not_found "$BASE/admin/markets/mkt_00000000000000000000000000000063"
step "fetch the catalog" 200 - "$BASE/admin/catalogs/$CATALOG"
contains "with its price list" "\"price_list\":\"$LIST\""
step "the catalog's products, a page of one" 200 - "$BASE/admin/catalogs/$CATALOG/products?limit=1"
contains "there are more" '"has_more":true'
step "an unknown catalog's products" 404 catalog_not_found \
    "$BASE/admin/catalogs/cat_00000000000000000000000000000063/products"
step "list price lists" 200 - "$BASE/admin/price-lists"
contains "with its adjustment" '"adjustment_bps":-1000'
step "fetch the price list" 200 - "$BASE/admin/price-lists/$LIST"
step "its fixed prices" 200 - "$BASE/admin/price-lists/$LIST/fixed-prices"
contains "the cup at 200" "{\"variant\":\"$CUP_VAR\",\"price\":{\"amount\":200,\"currency\":\"EUR\"}"
step "rename the market" 200 - -X PATCH "$BASE/admin/markets/$MARKET" -H "$JSON" -d '{"name":"Euro area"}'
contains "renamed" '"name":"Euro area"'
step "and back" 200 - -X PATCH "$BASE/admin/markets/$MARKET" -H "$JSON" -d '{"name":"Eurozone"}'
step "deepen the discount" 200 - -X PATCH "$BASE/admin/price-lists/$LIST" -H "$JSON" -d '{"adjustment_bps":-2000}'
contains "-20%" '"adjustment_bps":-2000'
step "back to -10%" 200 - -X PATCH "$BASE/admin/price-lists/$LIST" -H "$JSON" -d '{"adjustment_bps":-1000}'
step "fixed prices name their product" 200 - "$BASE/admin/price-lists/$LIST/fixed-prices"
contains "the cup" '"product":{"id":'
step "list API keys" 200 - "$BASE/admin/api-keys"
contains "the storefront's key among them" '"kind":"publishable"'
step "countries for pickers" 200 - "$BASE/admin/countries"
contains "Serbia among them" '{"code":"RS","name":"Serbia"}'

step "create cart" 201 - -X POST "$BASE/storefront/carts" -H "$JSON" -d '{"country":"HR"}'
contains "cart ships to Croatia" '"country":"HR"'
contains "cart names its market and currency" "\"market\":\"$MARKET\",\"currency\":\"EUR\""
CART=$(id)
step "a mug not published to the key's channel is refused" 422 product_not_published \
    -X POST "$BASE/storefront/carts/$CART/lines" -H "$JSON" -d "{\"variant\":\"$MUG_VAR\",\"quantity\":2}"
step "and isn't there for the storefront" 404 product_not_found "$BASE/storefront/products/$MUG"
step "publish mug and cup to the Online Store" 204 - -X POST "$BASE/admin/channels/$ONLINE_STORE/products" \
    -H "$JSON" -d "{\"products\":[\"$MUG\",\"$CUP\"]}"
step "the cup's page, priced in Croatia" 200 - "$BASE/storefront/products/by-handle/cup?country=HR"
contains "at its fixed price" '"price":{"amount":200,"currency":"EUR"},"available":true'
contains "sold there" '"sold_in_country":true,"currency":"EUR"'
step "the mug by ID, -10%" 200 - "$BASE/storefront/products/$MUG?country=HR"
contains "at 9.00" '"price":{"amount":900,"currency":"EUR"}'
step "the mug where the store doesn't sell" 200 - "$BASE/storefront/products/by-handle/mug?country=US"
contains "not sold there" '"sold_in_country":false,"currency":null'
step "a page without a country has no prices" 200 - "$BASE/storefront/products/by-handle/mug"
contains "none" '"price":null,"available":null'
step "an unknown handle" 404 product_not_found "$BASE/storefront/products/by-handle/no-such-thing?country=HR"
step "an unknown parameter" 400 invalid_query "$BASE/storefront/products/by-handle/mug?currency=EUR"
step "where the store sells" 200 - "$BASE/storefront/countries"
contains "Croatia, in euros" "{\"country\":\"HR\",\"name\":\"Croatia\",\"market\":\"$MARKET\",\"currency\":\"EUR\",\"minor_units\":2"
step "create an Outlet channel" 201 - -X POST "$BASE/admin/channels" -H "$JSON" \
    -d '{"name":"Outlet","kind":"custom"}'
OUTLET=$(id)
step "a channel's name is unique ignoring case" 409 channel_name_taken -X POST "$BASE/admin/channels" \
    -H "$JSON" -d '{"name":" outlet ","kind":"custom"}'
step "rename the outlet" 200 - -X PATCH "$BASE/admin/channels/$OUTLET" -H "$JSON" \
    -d '{"name":"Outlet store"}'
contains "renamed" '"name":"Outlet store","kind":"custom"'
step "an unknown channel" 404 channel_not_found "$BASE/admin/channels/chan_00000000000000000000000000000063"
step "sell the cup in both channels" 200 - -X PUT "$BASE/admin/products/$CUP/channels" -H "$JSON" \
    -d "{\"channels\":[\"$OUTLET\",\"$ONLINE_STORE\"]}"
step "the cup's channels" 200 - "$BASE/admin/products/$CUP"
contains "include the outlet" "\"$OUTLET\""
step "a product's channels must exist" 422 channel_not_found -X PUT "$BASE/admin/products/$CUP/channels" \
    -H "$JSON" -d '{"channels":["chan_00000000000000000000000000000063"]}'
step "take the cup out of the outlet" 204 - -X DELETE "$BASE/admin/channels/$OUTLET/products" \
    -H "$JSON" -d "{\"products\":[\"$CUP\"]}"
step "the cup's channels again" 200 - "$BASE/admin/products/$CUP"
contains "only the Online Store" "\"channels\":[\"$ONLINE_STORE\"]"
step "a storefront key for the outlet" 201 - -X POST "$BASE/admin/api-keys" -H "$JSON" \
    -d "{\"kind\":\"publishable\",\"name\":\"Outlet\",\"channel\":\"$OUTLET\"}"
ONLINE_SF_AUTH=("${SF_AUTH[@]}")
SF_AUTH=(-H "x-publishable-key: $(token)")
step "the outlet's storefront doesn't see the Online Store's cart" 404 cart_not_found \
    "$BASE/storefront/carts/$CART"
SF_AUTH=("${ONLINE_SF_AUTH[@]}")
step "add 2 mugs" 200 - -X POST "$BASE/storefront/carts/$CART/lines" -H "$JSON" \
    -d "{\"variant\":\"$MUG_VAR\",\"quantity\":2}"
contains "mug unit price is 900 (1000 - 10%)" '"unit_price":{"amount":900,"currency":"EUR"}'
contains "mug line is named, with its handle" '"product_title":"Mug","variant_title":null,"sku":null,"handle":"mug","image":null,"available_quantity":null,"status":"priced"'
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
echo "## extension calls (design note 0029)"
step "the courier's extension calls" 200 - "$BASE/admin/apps/app_courier/extension-calls"
contains "the failed quote is a shipping.rates call" '"point":"shipping.rates","version":1,'
contains "failed, unreachable" '"outcome":"failed","status":null,"error":"unreachable"'
contains "for the cart" "\"correlation\":\"$SHIP_CART\""
step "an unknown app's calls" 404 app_not_found "$BASE/admin/apps/app_none/extension-calls"
# A rate app when python3 is here (it is in CI; the host must run with
# HYPHA_ALLOW_PRIVATE_CALLBACKS=true to reach it): the courier's next quote is `ok`.
if command -v python3 >/dev/null 2>&1; then
    RATE_PORT=${E2E_RATE_PORT:-3198}
    python3 - "$RATE_PORT" <<'PY' &
import http.server, json, sys

class Rates(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers.get("content-length", 0))))
        currency = body["request"]["currency"]
        answer = json.dumps({"rates": [{"service": "express", "name": "Courier Express",
                                        "amount": {"amount": 700, "currency": currency}}]})
        self.send_response(200)
        self.send_header("content-type", "application/json")
        self.end_headers()
        self.wfile.write(answer.encode())

    def log_message(self, *args):
        pass

http.server.HTTPServer(("127.0.0.1", int(sys.argv[1])), Rates).serve_forever()
PY
    RATES=$!
    disown "$RATES"
    sleep 1
    step "point the courier at a rate app" 200 - -X PATCH "$BASE/admin/shipping-providers/app_courier" \
        -H "$JSON" -d "{\"callback_url\":\"http://127.0.0.1:$RATE_PORT/rates\"}"
    step "the offers, with the courier up" 200 - "$SHIP/shipping-options"
    contains "express costs 7 EUR" '"name":"Express","amount":{"amount":700,"currency":"EUR"}'
    step "the courier's latest call" 200 - "$BASE/admin/apps/app_courier/extension-calls?limit=1"
    contains "a shipping.rates call" '"point":"shipping.rates","version":1,'
    contains "ok, HTTP 200" '"outcome":"ok","status":200,"error":null'
    contains "for the cart" "\"correlation\":\"$SHIP_CART\""
    kill "$RATES" 2>/dev/null
    step "the courier down again" 200 - -X PATCH "$BASE/admin/shipping-providers/app_courier" \
        -H "$JSON" -d '{"callback_url":"http://127.0.0.1:9/rates"}'
fi
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

echo "## payment methods (design note 0027)"
METHODS="$BASE/admin/payment-methods"
step "a provider key that isn't one" 422 invalid_payment_provider -X POST "$METHODS" -H "$JSON" \
    -d '{"provider":"stripe","name":"Card"}'
step "a spore this build doesn't have (design note 0030)" 422 payment_provider_unknown -X POST "$METHODS" -H "$JSON" \
    -d '{"provider":"spore_nowhere","name":"Card"}'
step "settings the manual provider doesn't take" 422 invalid_payment_settings -X POST "$METHODS" -H "$JSON" \
    -d '{"provider":"manual","name":"Bank transfer","settings":{"iban":"HR12"}}'
step "bank transfer" 201 - -X POST "$METHODS" -H "$JSON" \
    -d '{"provider":"manual","name":"Bank transfer","instructions":"IBAN HR12 3456 7890. Quote the order number."}'
BANK=$(pm)
contains "enabled, first" '"enabled":true,"position":0'
step "cash on delivery, in the market" 201 - -X POST "$METHODS" -H "$JSON" \
    -d "{\"provider\":\"manual\",\"name\":\"Cash on delivery\",\"instructions\":\"Pay the courier.\",\"markets\":[\"$MARKET\"]}"
COD=$(pm)
contains "after it" '"position":1'
step "list the methods" 200 - "$METHODS"
contains "both, in position order" "\"items\":[{\"id\":\"$BANK\""
step "an unknown method" 404 payment_method_not_found "$METHODS/pm_00000000000000000000000000000000"
step "the cart's methods" 200 - "$BASE/storefront/carts/$KEYED/payment-methods"
contains "bank transfer, with its instructions" "{\"id\":\"$BANK\",\"name\":\"Bank transfer\",\"provider\":\"manual\",\"instructions\":\"IBAN HR12 3456 7890. Quote the order number.\"}"
contains "and cash on delivery" "\"id\":\"$COD\""
step "choose an unknown method" 422 payment_method_not_found -X PUT "$BASE/storefront/carts/$KEYED/payment-method" \
    -H "$JSON" -d '{"payment_method":"pm_00000000000000000000000000000000"}'
step "choose without the key (null clears; {} is refused)" 422 invalid_body -X PUT \
    "$BASE/storefront/carts/$KEYED/payment-method" -H "$JSON" -d '{}'
step "clear the choice with null" 200 - -X PUT "$BASE/storefront/carts/$KEYED/payment-method" \
    -H "$JSON" -d '{"payment_method":null}'
contains "no method chosen" '"payment_method":null'

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
step "pay by bank transfer" 200 - -X PUT "$BASE/storefront/carts/$KEYED/payment-method" -H "$JSON" \
    -d "{\"payment_method\":\"$BANK\"}"
contains "the cart has it" "\"payment_method\":\"$BANK\""
step "checkout" 201 - -X POST "$CHECKOUT" -H "$JSON" -H 'idempotency-key: order-1' \
    -d '{"expected_total":{"amount":1800,"currency":"EUR"}}'
ORDER=$(id)
contains "its payment is pending, with the instructions" '"payment":{"status":"pending","method_name":"Bank transfer","instructions":"IBAN HR12 3456 7890. Quote the order number.","amount":{"amount":1800,"currency":"EUR"}}'
contains "and the next action shows them" '"next_action":{"type":"instructions","instructions":"IBAN HR12 3456 7890. Quote the order number."}'
contains "the order is #1001" '"number":1001'
contains "it is pending and unfulfilled" '"financial_status":"pending","fulfillment_status":"unfulfilled"'
contains "it copies the line" '"product_title":"Mug","variant_title":null,"sku":null,"unit_price":{"amount":900,"currency":"EUR"}'
contains "its total is 1800" '"total":{"amount":1800,"currency":"EUR"},"lines"'
ORDER_TOKEN=$(sed -E 's/.*"access_token":"(hypha_ord_[0-9a-f]+)".*/\1/' <<<"$BODY")
step "the buyer reads the order with its token (design note 0024)" 200 - "$BASE/storefront/orders/$ORDER?token=$ORDER_TOKEN"
contains "it is the order" "\"id\":\"$ORDER\""
contains "with its payment" '"payment":{"status":"pending","method_name":"Bank transfer"'
step "without the right token it isn't there" 404 order_not_found "$BASE/storefront/orders/$ORDER?token=hypha_ord_00"
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
step "checkout again without the key" 201 - -X POST "$CHECKOUT" -H "$JSON" -d '{}'
contains "answers with its order (design note 0031)" "\"id\":\"$ORDER\""
step "get the checked-out cart" 200 - "$BASE/storefront/carts/$KEYED"
contains "the cart names its order" "\"order\":\"$ORDER\""
step "a completed cart takes no lines" 409 cart_completed -X POST "$BASE/storefront/carts/$KEYED/lines" \
    -H "$JSON" -d "{\"variant\":\"$MUG_VAR\",\"quantity\":1}"
step "get the order" 200 - "$BASE/admin/orders/$ORDER"
contains "it has a placed_at" '"placed_at":"20'
step "list orders" 200 - "$BASE/admin/orders?limit=1"
contains "the order is listed first" "\"items\":[{\"id\":\"$ORDER\""
step "orders by status and total (design note 0024)" 200 - "$BASE/admin/orders?financial_status=pending&currency=EUR&sort=total_desc"
contains "finds the order" "\"id\":\"$ORDER\""
step "a total sort without a currency" 422 sort_needs_currency "$BASE/admin/orders?sort=total_asc"
step "orders placed before 2000" 200 - "$BASE/admin/orders?placed_before=2000-01-01T00:00:00Z"
contains "none" '"items":[]'
step "a date that isn't one" 400 invalid_query "$BASE/admin/orders?placed_from=yesterday"
step "the admin sees the pending payment" 200 - "$BASE/admin/orders/$ORDER"
contains "with its method and instructions" '"status":"pending","refunded":{"amount":0,"currency":"EUR"},"reference":null,"attempt":null,"instructions":"IBAN HR12 3456 7890. Quote the order number."'
step "a method a pending payment uses" 409 payment_method_in_use -X DELETE "$METHODS/$BANK"
LONG_REFERENCE=$(printf 'x%.0s' $(seq 1 256))
step "a reference too long" 422 field_too_long -X POST "$BASE/admin/orders/$ORDER/paid" -H "$JSON" \
    -d "{\"reference\":\"$LONG_REFERENCE\"}"
step "mark it paid, with the bank's reference" 200 - -X POST "$BASE/admin/orders/$ORDER/paid" -H "$JSON" \
    -d '{"reference":"HR-2026-1001"}'
contains "it is paid" '"financial_status":"paid"'
contains "and so is its payment, with the reference" '"status":"paid","refunded":{"amount":0,"currency":"EUR"},"reference":"HR-2026-1001"'
step "mark it paid again" 200 - -X POST "$BASE/admin/orders/$ORDER/paid"
contains "still paid" '"financial_status":"paid"'
contains "the reference stays" '"reference":"HR-2026-1001"'
step "the buyer sees it paid" 200 - "$BASE/storefront/orders/$ORDER?token=$ORDER_TOKEN"
contains "paid" '"payment":{"status":"paid"'
step "an unknown order" 404 order_not_found "$BASE/admin/orders/ord_00000000000000000000000000000000"
step "an order ID of the wrong kind" 400 invalid_path "$BASE/admin/orders/$CART"
step "a new cart" 201 - -X POST "$BASE/storefront/carts" -H "$JSON" -d '{"country":"HR"}'
contains "an open cart has no order" '"order":null'
step "checkout an empty cart" 409 cart_empty -X POST "$BASE/storefront/carts/$(id)/checkout" -H "$JSON" -d '{}'
step "pay the shipping cart on delivery" 200 - -X PUT "$SHIP/payment-method" -H "$JSON" \
    -d "{\"payment_method\":\"$COD\"}"
step "checkout the shipping cart" 201 - -X POST "$SHIP/checkout" -H "$JSON" -d '{}'
contains "told to pay the courier" '"next_action":{"type":"instructions","instructions":"Pay the courier."}'
contains "the order charges shipping" '"shipping_total":{"amount":490,"currency":"EUR"},"tax_total":{"amount":0,"currency":"EUR"},"total":{"amount":1390'
contains "it copies the address" '"shipping_address":{"first_name":"Ana","last_name":"Horvat"'
contains "and the shipping line" '"code":"standard","name":"Standard","provider":null,"service":null,"reference":null,"amount":{"amount":490,"currency":"EUR"},"estimate":{"min_days":2,"max_days":4}'

echo "## payment and search apps (design note 0029, §3.1)"
step "a payment app that isn't installed" 422 payment_provider_unknown -X POST "$METHODS" -H "$JSON" \
    -d '{"provider":"app_pay","name":"Card"}'
step "an app that isn't installed searches nothing" 422 search_app_unavailable -X PATCH \
    "$BASE/admin/search/settings" -H "$JSON" -d '{"search_app":"app_find"}'
# Fake payment and search apps when python3 is here (the host must run with
# HYPHA_ALLOW_PRIVATE_CALLBACKS=true to reach them).
if command -v python3 >/dev/null 2>&1; then
    APPS_PORT=${E2E_APPS_PORT:-3199}
    python3 - "$APPS_PORT" "$MUG" <<'PY' &
import http.server, json, sys

MUG = sys.argv[2]
ANSWERS = {
    "/start": {"next_action": {"type": "client_widget", "element": "pay-card",
                               "props": {"client_secret": "pi_e2e_secret"}},
               "reference": "pi_e2e"},
    "/capture": {"status": "accepted", "reference": "ch_e2e"},
    "/void": {"status": "refused", "reason": "already captured"},
    "/search": {"hits": [{"product": MUG}], "total": 1},
}

class App(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        self.rfile.read(int(self.headers.get("content-length", 0)))
        answer = ANSWERS.get(self.path)
        self.send_response(200 if answer is not None else 404)
        self.send_header("content-type", "application/json")
        self.end_headers()
        self.wfile.write(json.dumps(answer or {"code": "unknown_extension"}).encode())

    def log_message(self, *args):
        pass

http.server.HTTPServer(("127.0.0.1", int(sys.argv[1])), App).serve_forever()
PY
    APPS=$!
    disown "$APPS"
    sleep 1
    APP_URL="http://127.0.0.1:$APPS_PORT"
    step "a payment app" 201 - -X POST "$BASE/admin/apps" -H "$JSON" -d '{"key":"app_pay","name":"Pay"}'
    step "a point this Hypha doesn't call" 422 unknown_extension_point -X PUT \
        "$BASE/admin/apps/app_pay/extension-points" -H "$JSON" \
        -d "{\"extension_points\":[{\"point\":\"payments.teleport\",\"version\":1,\"url\":\"$APP_URL/x\"}]}"
    step "it starts, captures and voids payments" 200 - -X PUT "$BASE/admin/apps/app_pay/extension-points" \
        -H "$JSON" -d "{\"extension_points\":[{\"point\":\"payments.start\",\"version\":2,\"url\":\"$APP_URL/start\"},{\"point\":\"payments.capture\",\"version\":1,\"url\":\"$APP_URL/capture\"},{\"point\":\"payments.void\",\"version\":1,\"url\":\"$APP_URL/void\"}]}"
    contains "by point" '"extension_points":[{"point":"payments.capture"'
    step "card, collected by the app" 201 - -X POST "$METHODS" -H "$JSON" -d '{"provider":"app_pay","name":"Card"}'
    CARD=$(pm)
    contains "the app's key is the provider" '"provider":"app_pay"'
    step "a cart paying by card" 201 - -X POST "$BASE/storefront/carts" -H "$JSON" -d '{"country":"HR"}'
    CARD_CART="$BASE/storefront/carts/$(id)"
    step "a mug in it" 200 - -X POST "$CARD_CART/lines" -H "$JSON" -d "{\"variant\":\"$MUG_VAR\",\"quantity\":1}"
    step "shipped to Zagreb" 200 - -X PUT "$CARD_CART/shipping-address" -H "$JSON" \
        -d '{"address":{"first_name":"Ana","last_name":"Horvat","line1":"Ilica 1","city":"Zagreb","country":"HR"}}'
    step "by standard" 200 - -X PUT "$CARD_CART/shipping/$(shgrp)" -H "$JSON" -d "{\"option\":\"$STANDARD\"}"
    step "with an email" 200 - -X PUT "$CARD_CART/email" -H "$JSON" -d '{"email":"card@example.hr"}'
    step "the card is offered" 200 - "$CARD_CART/payment-methods"
    contains "by the app" "{\"id\":\"$CARD\",\"name\":\"Card\",\"provider\":\"app_pay\""
    step "pay by card" 200 - -X PUT "$CARD_CART/payment-method" -H "$JSON" -d "{\"payment_method\":\"$CARD\"}"
    step "checkout before paying" 409 payment_required -X POST "$CARD_CART/checkout" -H "$JSON" -d '{}'
    step "start paying for the cart" 200 - -X POST "$CARD_CART/payment" -H "$JSON" \
        -d '{"return_url":"https://shop.example/back"}'
    contains "the app's widget is next" '"next_action":{"type":"client_widget","element":"pay-card","props":{"client_secret":"pi_e2e_secret"}},"payment_error":null'
    contains "pending, before any order" '"status":"pending"'
    CARD_PAYMENT=$(sed -E 's/.*"payment":"(pay_[0-9a-f]+)".*/\1/' <<<"$BODY")
    step "starting again resumes it" 200 - -X POST "$CARD_CART/payment" -H "$JSON" -d '{}'
    contains "the same payment" "\"payment\":\"$CARD_PAYMENT\""
    step "checkout before the app reports: waits" 409 payment_pending -X POST "$CARD_CART/checkout" \
        -H "$JSON" -d '{}'
    PAYMENTS="$BASE/admin/payments/$CARD_PAYMENT"
    step "the payment has no order yet" 200 - "$PAYMENTS"
    contains "a cart's, with the app's reference" '"order":null,"method":'
    step "the app reports it authorized" 200 - -X POST "$PAYMENTS/events" -H "$JSON" \
        -H 'idempotency-key: evt_e2e_1' -d '{"type":"authorized","reference":"pi_e2e","attempt":"pi_e2e"}'
    contains "which places the order" '"financial_status":"authorized"'
    CARD_ORDER=$(sed -E 's/.*"order":"(ord_[0-9a-f]+)".*/\1/' <<<"$BODY")
    replayed "the report again replays" -X POST "$PAYMENTS/events" -H "$JSON" \
        -H 'idempotency-key: evt_e2e_1' -d '{"type":"authorized","reference":"pi_e2e","attempt":"pi_e2e"}'
    step "the shopper's checkout gets that order" 201 - -X POST "$CARD_CART/checkout" -H "$JSON" \
        -d '{"return_url":"https://shop.example/back"}'
    contains "the same order" "\"id\":\"$CARD_ORDER\""
    contains "nothing more to do" '"next_action":null'
    step "the admin sees the app's payment" 200 - "$BASE/admin/orders/$CARD_ORDER"
    contains "authorized, with the app's reference" '"provider":"app_pay","amount":{"amount":1390,"currency":"EUR"},"status":"authorized","refunded":{"amount":0,"currency":"EUR"},"reference":"pi_e2e"'
    step "an authorized order can't be marked paid by hand" 409 payment_authorized -X POST \
        "$BASE/admin/orders/$CARD_ORDER/paid"
    step "staff void it, the provider refuses" 409 payment_refused -X POST "$PAYMENTS/void" -H "$JSON" -d '{}'
    contains "and says why" '"details":{"reason":"already captured"}'
    step "staff capture it" 202 - -X POST "$PAYMENTS/capture" -H "$JSON" -d '{}'
    contains "accepted; the payment waits for the app" '"reference":"ch_e2e","completed":false,"payment":{'
    contains "still authorized" '"status":"authorized"'
    step "the app reports it paid" 200 - -X POST "$PAYMENTS/events" -H "$JSON" \
        -H 'idempotency-key: evt_e2e_2' -d '{"type":"paid","occurred_at":"2026-10-18T12:00:00Z"}'
    contains "the order is paid" '"financial_status":"paid"'
    step "the payment alone" 200 - "$PAYMENTS"
    contains "with its order and status" "\"order\":\"$CARD_ORDER\",\"method\":\"$CARD\",\"method_name\":\"Card\",\"provider\":\"app_pay\""
    contains "paid" '"status":"paid"'
    step "another attempt's money, named, is a second collection" 409 payment_reference_mismatch \
        -X POST "$PAYMENTS/events" -H "$JSON" -d '{"type":"paid","reference":"pi_other","attempt":"pi_other"}'
    contains "naming both attempts" '"details":{"held":"pi_e2e","reported":"pi_other","status":"paid"}'
    step "an unknown payment alone" 404 payment_not_found "$BASE/admin/payments/pay_00000000000000000000000000000000"
    step "a refund the app doesn't answer" 409 payment_action_unsupported -X POST "$PAYMENTS/refund" \
        -H "$JSON" -d '{"amount":300}'
    step "a refund of more than was paid" 422 invalid_refund_amount -X POST "$PAYMENTS/events" -H "$JSON" \
        -d '{"type":"partially_refunded","amount":5000}'
    step "the app reports a partial refund" 200 - -X POST "$PAYMENTS/events" -H "$JSON" \
        -H 'idempotency-key: evt_e2e_3' -d '{"type":"partially_refunded","amount":300}'
    contains "partly refunded" '"refunded":{"amount":300,"currency":"EUR"}'
    contains "and so is the order" '"financial_status":"partially_refunded"'
    step "an unknown payment" 404 payment_not_found -X POST \
        "$BASE/admin/payments/pay_00000000000000000000000000000000/events" -H "$JSON" -d '{"type":"paid"}'
    step "the app's calls" 200 - "$BASE/admin/apps/app_pay/extension-calls"
    contains "the capture" '"point":"payments.capture","version":1,'
    contains "and the start, for its payment" "\"point\":\"payments.start\",\"version\":2,"
    contains "with the payment" "\"correlation\":\"$CARD_PAYMENT\""

    step "a search app" 201 - -X POST "$BASE/admin/apps" -H "$JSON" -d '{"key":"app_find","name":"Find"}'
    step "the payment app doesn't search" 422 search_app_unavailable -X PATCH \
        "$BASE/admin/search/settings" -H "$JSON" -d '{"search_app":"app_pay"}'
    step "it answers search.query" 200 - -X PUT "$BASE/admin/apps/app_find/extension-points" -H "$JSON" \
        -d "{\"extension_points\":[{\"point\":\"search.query\",\"version\":1,\"url\":\"$APP_URL/search\"}]}"
    step "searches go to the app" 200 - -X PATCH "$BASE/admin/search/settings" -H "$JSON" \
        -d '{"search_app":"app_find"}'
    contains "the setting is kept" '"search_app":"app_find"'
    step "the app answers a storefront search" 200 - "$BASE/storefront/search?country=HR&q=nothing"
    contains "the mug it found" "\"items\":[{\"product\":\"$MUG\""
    contains "one in all, no price range" '"total":1,'
    kill "$APPS" 2>/dev/null
    step "the app is down: the built-in engine answers" 200 - "$BASE/storefront/search?country=HR&q=nothing"
    contains "finding nothing" '"items":[],"total":0'
    step "back to the built-in engine" 200 - -X PATCH "$BASE/admin/search/settings" -H "$JSON" \
        -d '{"search_app":null}'
    contains "no search app" '"search_app":null'
fi

step "archive mug" 200 - -X PATCH "$BASE/admin/products/$MUG/status" -H "$JSON" -d '{"status":"archived"}'
step "get cart" 200 - "$BASE/storefront/carts/$CART"
contains "mug line is unavailable" '"status":"unavailable","reason_code":"product_not_active"'
contains "unavailable mug keeps its name" '"product_title":"Mug","variant_title":null,"sku":null,"handle":"mug","image":null,"available_quantity":null,"status":"unavailable"'
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
step "publish book" 204 - -X POST "$BASE/admin/channels/$ONLINE_STORE/products" -H "$JSON" \
    -d "{\"products\":[\"$BOOK\"]}"
step "create tax class" 201 - -X POST "$BASE/admin/tax-classes" -H "$JSON" -d '{"name":"Books"}'
BOOKS=$(id)
step "put book in it" 204 - -X PUT "$BASE/admin/tax-assignments" -H "$JSON" \
    -d "{\"products\":[\"$BOOK\"],\"tax_class\":\"$BOOKS\"}"
step "create Croatian tax region (25%)" 201 - -X POST "$BASE/admin/tax-regions" -H "$JSON" \
    -d '{"country":"HR","default_rate_ppm":250000}'
REGION=$(id)
step "list tax classes" 200 - "$BASE/admin/tax-classes"
contains "books among them" '"name":"Books"'
step "list tax regions" 200 - "$BASE/admin/tax-regions"
contains "Croatia among them" '"country":"HR"'
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
step "publish lamp" 204 - -X POST "$BASE/admin/channels/$ONLINE_STORE/products" -H "$JSON" \
    -d "{\"products\":[\"$LAMP\"]}"
step "lamp has no stock set up" 200 - "$BASE/admin/variants/$LAMP_VAR/inventory"
contains "so it is untracked" '"id":null,'
contains "reported as tracked: false" '"tracked":false'
step "the lamp's stock, every variant at once" 200 - "$BASE/admin/products/$LAMP/inventory"
contains "one untracked variant" "\"items\":[{\"id\":null,\"variant\":\"$LAMP_VAR\",\"tracked\":false"
step "the setup checklist" 200 - "$BASE/admin/setup/checklist"
contains "a market exists by now" '"market":true'
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
contains "and says 5 can be bought" '"available_quantity":5'
step "stop tracking lamps" 200 - -X PATCH "$BASE/admin/inventory-items/$LAMP_ITEM" -H "$JSON" -d '{"tracked":false}'
step "add 100 untracked lamps" 200 - -X POST "$BASE/storefront/carts/$STOCK_CART/lines" -H "$JSON" \
    -d "{\"variant\":\"$LAMP_VAR\",\"quantity\":100}"
contains "untracked lamps have no limit" '"available_quantity":null'
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
step "customers by text, by name (design note 0024)" 200 - "$BASE/admin/customers?q=horvat&sort=name_asc"
contains "finds her" "\"items\":[{\"id\":\"$CUSTOMER\""
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
# (Checkout counts against the cart's quote limit, so this is a cart that's barely used.)
step "checkout without a payment method" 409 payment_method_required -X POST \
    "$BASE/storefront/carts/$SHOP_CART/checkout" -H "$JSON" -d '{}'
step "cash on delivery" 200 - -X PUT "$BASE/storefront/carts/$SHOP_CART/payment-method" -H "$JSON" \
    -d "{\"payment_method\":\"$COD\"}"
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
step "cash on delivery again" 200 - -X PUT "$BASE/storefront/carts/$AGAIN_CART/payment-method" -H "$JSON" \
    -d "{\"payment_method\":\"$COD\"}"
step "disable cash on delivery" 200 - -X PATCH "$METHODS/$COD" -H "$JSON" -d '{"enabled":false}'
contains "it's disabled" '"enabled":false'
step "checkout with a method no longer offered" 409 payment_method_unavailable -X POST \
    "$BASE/storefront/carts/$AGAIN_CART/checkout" -H "$JSON" -d '{}'
step "the cart isn't offered it" 200 - "$BASE/storefront/carts/$AGAIN_CART/payment-methods"
contains "only the bank transfer" "{\"items\":[{\"id\":\"$BANK\""
step "nor can it choose it" 422 payment_method_unavailable -X PUT "$BASE/storefront/carts/$AGAIN_CART/payment-method" \
    -H "$JSON" -d "{\"payment_method\":\"$COD\"}"
step "enable it again, first" 200 - -X PATCH "$METHODS/$COD" -H "$JSON" -d '{"enabled":true,"position":0}'
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
step "pages stop 1000 deep" 422 search_too_deep "$BASE/storefront/search?country=HR&cursor=$(printf '{"v":1,"after":"1000","filters":[["country","HR"]]}' | base64 | tr '+/' '-_' | tr -d '=\n')"
step "search settings" 200 - "$BASE/admin/search/settings"
contains "the defaults" '{"split_by":[],"facets":[],"search_app":null}'
step "split listings by colour" 200 - -X PATCH "$BASE/admin/search/settings" -H "$JSON" \
    -d '{"split_by":["Color"],"facets":[{"key":"vendor","label":"Brand"},{"key":"option.color"}]}'
step "at most 3 splits" 422 too_many_splits -X PATCH "$BASE/admin/search/settings" -H "$JSON" \
    -d '{"split_by":["a","b","c","d"]}'
step "back to one listing per product" 200 - -X PATCH "$BASE/admin/search/settings" -H "$JSON" \
    -d '{"split_by":[],"facets":[]}'
step "rebuild everything" 202 - -X POST "$BASE/admin/search/reindex"
wait_for "the rebuild finishes" "$BASE/admin/search/status" '"queued":0'

echo "## categories (design note 0022)"
step "a top-level category" 201 - -X POST "$BASE/admin/categories" -H "$JSON" \
    -d '{"name":"Home","handle":"home","visible":true,"description":"<p>For the home</p><script>x()</script>"}'
HOME_CAT=$(id)
contains "its description sanitised" '"description":"<p>For the home</p>"'
step "a subcategory" 201 - -X POST "$BASE/admin/categories" -H "$JSON" \
    -d "{\"name\":\"Lighting\",\"handle\":\"lighting\",\"parent\":\"$HOME_CAT\",\"visible\":true}"
LIGHTING=$(id)
contains "with its breadcrumb" "\"ancestors\":[{\"id\":\"$HOME_CAT\""
step "a hidden one" 201 - -X POST "$BASE/admin/categories" -H "$JSON" \
    -d '{"name":"Clearance","handle":"clearance","visible":false}'
CLEARANCE=$(id)
step "visible is required" 422 invalid_body -X POST "$BASE/admin/categories" -H "$JSON" -d '{"name":"X","handle":"x"}'
step "a taken handle" 409 category_handle_taken -X POST "$BASE/admin/categories" -H "$JSON" \
    -d '{"name":"Again","handle":"home","visible":true}'
step "under its own child" 422 category_cycle -X PATCH "$BASE/admin/categories/$HOME_CAT" -H "$JSON" \
    -d "{\"parent\":\"$LIGHTING\"}"
step "the tree" 200 - "$BASE/admin/categories"
contains "nested" "\"children\":[{\"id\":\"$LIGHTING\""
step "the lamp is lighting, and on clearance" 200 - -X PUT "$BASE/admin/products/$LAMP/categories" -H "$JSON" \
    -d "{\"categories\":[\"$LIGHTING\",\"$CLEARANCE\"]}"
step "the lamp names its categories" 200 - "$BASE/admin/products/$LAMP"
contains "both" '"categories":['
step "the cup is in Home" 204 - -X POST "$BASE/admin/categories/$HOME_CAT/products" -H "$JSON" \
    -d "{\"products\":[\"$CUP\"]}"
step "Home's own products" 200 - "$BASE/admin/categories/$HOME_CAT/products"
contains "the cup" '"handle":"cup"'
step "a parent can't be deleted" 409 category_has_children -X DELETE "$BASE/admin/categories/$HOME_CAT"
step "the storefront's tree" 200 - "$BASE/storefront/categories"
contains "without the hidden one" '"handle":"home"'
step "a category page" 200 - "$BASE/storefront/categories/by-handle/lighting"
contains "with its breadcrumb" '"ancestors":[{'
step "a hidden category page" 404 category_not_found "$BASE/storefront/categories/by-handle/clearance"
wait_for "Home lists the lamp from Lighting" "$BASE/storefront/search?country=HR&category=$HOME_CAT" '"handle":"lamp"'
step "a hidden category isn't searchable" 404 category_not_found "$BASE/storefront/search?country=HR&category=$CLEARANCE"
step "delete the hidden one" 204 - -X DELETE "$BASE/admin/categories/$CLEARANCE"
step "it's gone" 404 category_not_found "$BASE/admin/categories/$CLEARANCE"

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
contains "its cash-on-delivery payment is voided" '"status":"voided"'
contains "and so is the order's payment status" '"financial_status":"voided"'
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

echo "## notifications (design note 0028)"
# The shop order's buyer was told at each step the fulfilment section asked for.
mail "the guest's order confirmation arrived" guest@example.hr "confirmed"
mail "and that it shipped" guest@example.hr "on its way"
mail "and the new tracking" guest@example.hr "Tracking updated"
mail "and the cancellation" guest@example.hr "cancelled"
NOTIFICATIONS="$BASE/admin/notifications?order=$SHOP_ORDER"
wait_for "the order's emails are recorded" "$NOTIFICATIONS" '"template":"order_cancelled"'
contains "the confirmation too" '"template":"order_confirmation"'
contains "sent" '"status":"sent"'
step "the order's confirmation" 200 - "$NOTIFICATIONS&template=order_confirmation"
CONFIRMATION=$(sed -E 's/.*"id":"(ntf_[0-9a-f]+)".*/\1/' <<<"$BODY")
step "resend it" 201 - -X POST "$BASE/admin/notifications/$CONFIRMATION/resend"
contains "a new message, about no event" '"event_id":null'
contains "queued for the send job" '"status":"queued"'
RESENT=$(sed -E 's/^\{"id":"(ntf_[0-9a-f]+)".*/\1/' <<<"$BODY")
mail "the resent confirmation arrived" guest@example.hr "confirmed"
wait_for "and is recorded as sent" "$BASE/admin/notifications/$RESENT" '"status":"sent"'
step "one message" 200 - "$BASE/admin/notifications/$CONFIRMATION"
step "an unknown message" 404 notification_not_found \
    "$BASE/admin/notifications/ntf_00000000000000000000000000000063"
step "an unknown filter" 400 invalid_query "$BASE/admin/notifications?template=nope"
step "the templates" 200 - "$BASE/admin/notification-templates"
contains "the layout first" '"items":[{"key":"layout"'
step "one template" 200 - "$BASE/admin/notification-templates/order_shipped"
contains "with its variables" '"variables":{'
step "an unknown template" 404 template_unknown "$BASE/admin/notification-templates/nope"
TEMPLATES="$BASE/admin/notification-templates/order_shipped"
BROKEN='{"subject":"Shipped","html":"{% extends \"layout\" %}{% block content %}{{ order.nope }}{% endblock %}"}'
GOOD='{"subject":"Shipped: {{ order.number }}","html":"{% extends \"layout\" %}{% block content %}<p>{{ order.number }}</p>{% endblock %}"}'
step "save a template that doesn't render" 422 template_invalid -X PUT "$TEMPLATES" -H "$JSON" -d "$BROKEN"
contains "saying where" '"part":"html"'
step "preview it" 422 template_invalid -X POST "$TEMPLATES/preview" -H "$JSON" -d "$BROKEN"
step "preview one that does" 200 - -X POST "$TEMPLATES/preview" -H "$JSON" -d "$GOOD"
contains "rendered against the sample" '"subject":"Shipped: '
step "save it" 200 - -X PUT "$TEMPLATES" -H "$JSON" -d "$GOOD"
contains "customized" '"customized":true'
step "turn off an invite" 422 template_required -X PUT \
    "$BASE/admin/notification-templates/staff_invite" -H "$JSON" \
    -d '{"subject":"Hi","html":"{% extends \"layout\" %}","enabled":false}'
step "revert it" 200 - -X DELETE "$TEMPLATES"
contains "the default again" '"customized":false'
if [[ $SENDER == log ]]; then
    step "a test send with the log sender" 409 email_not_configured -X POST "$TEMPLATES/test" \
        -H "$JSON" -d "$GOOD"
else
    step "a test send" 200 - -X POST "$TEMPLATES/test" -H "$JSON" -d "$GOOD"
    contains "to the owner" "\"sent_to\":\"$OWNER_EMAIL\""
    mail "the test arrived" "$OWNER_EMAIL" "Test"
fi
step "branding" 200 - -X PATCH "$BASE/admin/notification-settings" -H "$JSON" \
    -d '{"accent":"#0a7d3b","footer":"Ilica 1, Zagreb","sender_name":"Hypha e2e"}'
contains "saved" '"accent":"#0a7d3b"'
step "clear the footer" 200 - -X PATCH "$BASE/admin/notification-settings" -H "$JSON" -d '{"footer":null}'
contains "cleared" '"footer":null'
step "a colour that isn't one" 422 invalid_colour -X PATCH "$BASE/admin/notification-settings" \
    -H "$JSON" -d '{"accent":"green"}'

echo "## apps and webhooks (design note 0016)"
# A webhook receiver when python3 is here (it is in CI): the host must run with
# HYPHA_ALLOW_PRIVATE_CALLBACKS=true to reach it. Without one, the delivery fails and the
# script checks that instead.
HOOK_PORT=${E2E_HOOK_PORT:-3199}
HOOKS_FILE=$(mktemp)
HOOKS=''
# The same server is an app's site for the OAuth steps (design note 0029): it serves the
# app's manifest at /hypha-app.json.
APP_BASE="http://127.0.0.1:$HOOK_PORT"
MANIFEST_FILE=$(mktemp)
cat >"$MANIFEST_FILE" <<JSON
{"key":"app_e2e_helper","name":"E2E helper","version":"1.0.0","publisher":"E2E",
 "app_url":"$APP_BASE","redirect_uris":["/callback"],
 "scopes":["orders:read","customers:read","customer_data:email"],
 "scope_reasons":{"customer_data:email":"receipts"},"privacy_url":"/privacy",
 "webhooks":{"url":"/hooks","events":["order.*"]},
 "extension_points":[{"point":"search.query","version":1,"url":"/search"}]}
JSON
if command -v python3 >/dev/null 2>&1; then
    python3 - "$HOOK_PORT" "$HOOKS_FILE" "$MANIFEST_FILE" <<'PY' &
import http.server, sys

port, out, manifest = int(sys.argv[1]), sys.argv[2], sys.argv[3]

class Hook(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path != "/hypha-app.json":
            self.send_response(404)
            self.end_headers()
            return
        with open(manifest, "rb") as f:
            body = f.read()
        self.send_response(200)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self):
        body = self.rfile.read(int(self.headers.get("content-length", 0)))
        with open(out, "ab") as f:
            event = self.headers.get("hypha-event-type", "").encode()
            store = self.headers.get("hypha-store", "").encode()
            f.write(event + b" " + store + b" " + body + b"\n")
        self.send_response(204)
        self.end_headers()

    def log_message(self, *args):
        pass

http.server.HTTPServer(("127.0.0.1", port), Hook).serve_forever()
PY
    HOOKS=$!
    # Not a job of this shell any more, so killing it at the end prints nothing.
    disown "$HOOKS"
    trap 'kill $HOOKS 2>/dev/null; rm -f "$HOOKS_FILE" "$MANIFEST_FILE"' EXIT
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
contains "the courier from shipping is one" '"key":"app_courier","kind":"app","name":"Courier","enabled":true'
step "get the app" 200 - "$BASE/admin/apps/app_erp"
contains "with its webhook, without its secret" "\"webhook\":{\"url\":\"$HOOK_URL\",\"timeout_ms\":2000}"
step "an unknown app" 404 app_not_found "$BASE/admin/apps/app_none"
# Spores' settings (design note 0030, §8): the stock build has no spores, so only the
# refusals are checked here; the route tests build a host with one.
step "settings of a spore this build doesn't have" 404 app_not_found "$BASE/admin/apps/spore_none/settings"
step "an app keeps its own settings" 404 app_not_found "$BASE/admin/apps/app_erp/settings"
step "saving them for no spore" 404 app_not_found -X PUT "$BASE/admin/apps/spore_none/settings" \
    -H "$JSON" -d '{"settings":{}}'
step "a spore's block, without the spore" 404 app_not_found "$BASE/admin/app-extensions/blocks/spore_none/notes"
step "nor routes under a spore's prefix" 404 route_not_found "$BASE/admin/spores/none/anything"
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
    if grep -Eq '^product\.created store_[0-9a-f]{32} ' "$HOOKS_FILE"; then
        echo "ok   with the store in Hypha-Store"
    else
        echo "FAIL with the store in Hypha-Store: $(cat "$HOOKS_FILE")"
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
echo "## apps through OAuth (design note 0029)"
# Needs the receiver above (it serves the manifest) and a host with HYPHA_ADMIN_URL set
# (the consent page's address; nothing is served there, the script plays the admin).
if [[ -n $HOOKS ]]; then
    # PKCE (S256): a random verifier and its challenge, base64url without padding.
    VERIFIER=$(openssl rand -hex 32)
    CHALLENGE=$(printf %s "$VERIFIER" | openssl dgst -sha256 -binary | openssl base64 -A | tr '+/' '-_' | tr -d '=')
    CLIENT="$APP_BASE/hypha-app.json"
    REDIRECT="$APP_BASE/callback"
    # authorize WANT_STATUS EXTRA_CURL_ARGS...: GET /oauth/authorize; leaves Location in $LOCATION.
    authorize() {
        local name=$1 want=$2
        shift 2
        local out
        out=$(curl -s -o /dev/null -w '%{http_code} %{redirect_url}' --get "$BASE/oauth/authorize" \
            --data-urlencode response_type=code --data-urlencode "client_id=$CLIENT" \
            --data-urlencode state=e2e-state --data-urlencode "code_challenge=$CHALLENGE" \
            --data-urlencode code_challenge_method=S256 "$@")
        LOCATION=${out#* }
        if [[ ${out%% *} == "$want" ]]; then
            echo "ok   $name"
        else
            echo "FAIL $name: expected HTTP $want, got $out"
            FAILED=$((FAILED + 1))
        fi
    }
    step "a redirect URI the manifest doesn't list is a page" 400 invalid_redirect_uri --get "$BASE/oauth/authorize" \
        --data-urlencode response_type=code --data-urlencode "client_id=$CLIENT" \
        --data-urlencode "redirect_uri=$APP_BASE/elsewhere" --data-urlencode "code_challenge=$CHALLENGE" \
        --data-urlencode code_challenge_method=S256
    authorize "a scope the manifest lacks goes back to the app" 302 --data-urlencode "redirect_uri=$REDIRECT" \
        --data-urlencode scope=products:write
    if [[ $LOCATION == "$REDIRECT?"*"error=invalid_scope"*"state=e2e-state"* ]]; then
        echo "ok   with error=invalid_scope and the state"
    else
        echo "FAIL with error=invalid_scope and the state: $LOCATION"
        FAILED=$((FAILED + 1))
    fi
    authorize "authorize sends the browser to the consent page" 302 --data-urlencode "redirect_uri=$REDIRECT" \
        --data-urlencode "scope=orders:read customers:read"
    REQUEST=$(sed -E 's/.*request=(oareq_[0-9a-f]+).*/\1/' <<<"$LOCATION")
    step "the consent page reads the request" 200 - "$BASE/admin/oauth/requests/$REQUEST"
    contains "what's asked, by area" '"groups":[{"area":"customers","scopes":["customers:read"]},{"area":"orders","scopes":["orders:read"]}]'
    contains "no personal data asked" '"personal_data":[]'
    step "the owner approves" 200 - -X POST "$BASE/admin/oauth/requests/$REQUEST/approve"
    CODE=$(sed -E 's/.*[?&]code=(hypha_ac_[0-9a-f]+).*/\1/' <<<"$BODY")
    step "a request is answered once" 409 oauth_request_decided -X POST "$BASE/admin/oauth/requests/$REQUEST/approve"
    step "approved, the app hears nothing before it has its secret" 200 - \
        "$BASE/admin/apps/app_e2e_helper/deliveries"
    contains "no deliveries yet" '"items":[]'
    exchange() {
        curl -s -w '\n%{http_code}' -X POST "$BASE/oauth/token" \
            --data-urlencode grant_type=authorization_code --data-urlencode "code=$CODE" \
            --data-urlencode "code_verifier=$1" --data-urlencode "client_id=$CLIENT" \
            --data-urlencode "redirect_uri=$REDIRECT"
    }
    oauth_step() {
        local name=$1 want=$2 out=$3
        local status=${out##*$'\n'}
        BODY=${out%$'\n'*}
        if [[ $status == "$want" ]]; then echo "ok   $name"; else
            echo "FAIL $name: expected HTTP $want, got $status: $BODY"
            FAILED=$((FAILED + 1))
        fi
    }
    oauth_step "a wrong PKCE verifier is refused" 400 "$(exchange "$(openssl rand -hex 32)")"
    contains "as invalid_grant" '"error":"invalid_grant"'
    oauth_step "the code for tokens" 200 "$(exchange "$VERIFIER")"
    contains "with the app's secret, once" '"app_secret":"hypha_as_'
    contains "the store it's installed in" '"store":"store_'
    contains "and the API's base URL" "\"issuer\":\"$BASE\""
    APP_TOKEN_BODY=$BODY
    wait_for "now it hears it's installed" "$BASE/admin/apps/app_e2e_helper/deliveries?status=succeeded" \
        '"event_type":"app.installed"'
    BODY=$APP_TOKEN_BODY
    APP_TOKEN=$(sed -E 's/.*"access_token":"(hypha_at_[0-9a-f]+)".*/\1/' <<<"$BODY")
    REFRESH=$(sed -E 's/.*"refresh_token":"(hypha_rt_[0-9a-f]+)".*/\1/' <<<"$BODY")
    OWNER_AUTH=("${AUTH[@]}")
    AUTH=(-H "authorization: Bearer $APP_TOKEN")
    step "the app reads customers" 200 - "$BASE/admin/customers"
    contains "without their personal data" '"redacted":["first_name","last_name","email","phone"]'
    if [[ $BODY == *"ana@example.hr"* ]]; then
        echo "FAIL no email reaches the app: $BODY"
        FAILED=$((FAILED + 1))
    else
        echo "ok   no email reaches the app"
    fi
    step "searching by email needs customer_data:email" 403 app_not_authorized "$BASE/admin/customers?q=ana"
    step "nor may it change products" 403 app_not_authorized -X POST "$BASE/admin/products" -H "$JSON" \
        -d '{"handle":"app-mug","title":"Mug","price":{"amount":1,"currency":"EUR"}}'
    AUTH=("${OWNER_AUTH[@]}")
    oauth_step "refresh rotates" 200 "$(curl -s -w '\n%{http_code}' -X POST "$BASE/oauth/token" \
        --data-urlencode grant_type=refresh_token --data-urlencode "refresh_token=$REFRESH")"
    NEW_TOKEN=$(sed -E 's/.*"access_token":"(hypha_at_[0-9a-f]+)".*/\1/' <<<"$BODY")
    contains "a refresh names the store too" '"store":"store_'
    oauth_step "the old refresh token again is theft" 400 "$(curl -s -w '\n%{http_code}' -X POST "$BASE/oauth/token" \
        --data-urlencode grant_type=refresh_token --data-urlencode "refresh_token=$REFRESH")"
    AUTH=(-H "authorization: Bearer $NEW_TOKEN")
    step "and every token of the app is revoked" 401 invalid_token "$BASE/admin/orders"
    AUTH=("${OWNER_AUTH[@]}")
    step "the app's grant" 200 - "$BASE/admin/apps/app_e2e_helper"
    contains "installed, with its scopes" '"installed":true,"grant":{"scopes":["customers:read","orders:read"]'
    step "its extension points are its manifest's" 200 - "$BASE/admin/apps/app_e2e_helper/extension-points"
    contains "resolved against its app_url" "\"url\":\"$APP_BASE/search\""
    step "which staff don't set" 409 endpoints_from_manifest -X PUT \
        "$BASE/admin/apps/app_e2e_helper/extension-points" -H "$JSON" -d '{"extension_points":[]}'
    step "nor is it called without products:read" 422 search_app_unavailable -X PATCH \
        "$BASE/admin/search/settings" -H "$JSON" -d '{"search_app":"app_e2e_helper"}'
    step "uninstall it" 204 - -X DELETE "$BASE/admin/apps/app_e2e_helper"
    wait_for "it hears it last" "$BASE/admin/apps/app_e2e_helper/deliveries?status=succeeded" '"event_type":"app.uninstalled"'
    step "delete it for good" 204 - -X DELETE "$BASE/admin/apps/app_e2e_helper"
    step "the OAuth metadata (RFC 8414)" 200 - "$BASE/.well-known/oauth-authorization-server"
    contains "PKCE with S256" '"code_challenge_methods_supported":["S256"]'
else
    echo "skip OAuth steps: no python3 for the app's site"
fi

echo
echo "## app UI in any admin and storefront (design note 0029, §5-§8)"
# A second local app (python3, like the receiver above) with pages, blocks, actions,
# settings, both proxies and a storefront element; installed through OAuth.
UI_PORT=${E2E_UI_PORT:-3198}
UI_BASE="http://127.0.0.1:$UI_PORT"
UI_MANIFEST=$(mktemp)
write_ui_manifest() {
    cat >"$UI_MANIFEST" <<JSON
{"key":"app_e2e_ui","name":"E2E UI","version":"1.0.0","app_url":"$UI_BASE",
 "redirect_uris":["/callback"],"scopes":$1,"privacy_url":"/privacy",
 "settings":{"schema":{"type":"object"},"secret_fields":["api_key"]},
 "admin":{"pages":[{"id":"dashboard","title":"Dashboard","url":"/admin","nav":"apps"}],
  "blocks":[{"id":"payment","target":"order.payment","url":"/blocks/payment"},
            {"id":"bad","target":"order.details","url":"/blocks/bad"}],
  "actions":[{"id":"capture","target":"payment","label":"Capture","url":"/actions/capture"},
             {"id":"refund","target":"payment","label":"Refund","point":"payments.refund"}],
  "proxy":"/admin-api"},
 "storefront":{"elements":[{"id":"pay","target":"checkout.payment","module":"/el.js","tag":"e2e-pay"}],
  "proxy":"/sf"}}
JSON
}
write_ui_manifest '["orders:read","products:read"]'
UI_APP=''
if command -v python3 >/dev/null 2>&1 && [[ -n $HOOKS ]]; then
    python3 - "$UI_PORT" "$UI_MANIFEST" <<'PY' &
import http.server, json, sys

port, manifest = int(sys.argv[1]), sys.argv[2]

class App(http.server.BaseHTTPRequestHandler):
    def answer(self, status, body, kind="application/json"):
        data = body if isinstance(body, bytes) else json.dumps(body, separators=(",", ":")).encode()
        self.send_response(status)
        self.send_header("content-type", kind)
        self.send_header("set-cookie", "app=1")
        self.send_header("content-length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def echo(self, status):
        length = int(self.headers.get("content-length", 0))
        body = self.rfile.read(length).decode() if length else ""
        hypha = {k.lower(): v for k, v in self.headers.items() if k.lower().startswith("hypha-")}
        self.answer(status, {"method": self.command, "path": self.path, "headers": hypha, "body": body})

    def route(self):
        if self.path == "/hypha-app.json":
            with open(manifest, "rb") as f:
                return self.answer(200, f.read())
        if self.path.startswith("/blocks/payment"):
            return self.answer(200, {"version": 1, "root": {"type": "card", "title": "Paid",
                "children": [{"type": "badge", "value": "captured", "tone": "success"}]}})
        if self.path.startswith("/blocks/bad"):
            return self.answer(200, {"version": 2, "root": {"type": "divider"}})
        if self.path == "/actions/capture":
            self.rfile.read(int(self.headers.get("content-length", 0)))
            return self.answer(200, {"message": "Captured", "refresh": True})
        if self.path.startswith("/admin-api/") or self.path.startswith("/sf/"):
            return self.echo(201 if self.command == "POST" else 200)
        self.answer(404, {"code": "not_found"})

    do_GET = do_POST = do_PUT = do_PATCH = do_DELETE = route

    def log_message(self, *args):
        pass

http.server.HTTPServer(("127.0.0.1", port), App).serve_forever()
PY
    UI_APP=$!
    disown "$UI_APP"
    trap 'kill $HOOKS $UI_APP 2>/dev/null; rm -f "$HOOKS_FILE" "$MANIFEST_FILE" "$UI_MANIFEST"' EXIT
    sleep 1
fi
if [[ -n $UI_APP ]]; then
    UI_CLIENT="$UI_BASE/hypha-app.json"
    UI_VERIFIER=$(openssl rand -hex 32)
    UI_CHALLENGE=$(printf %s "$UI_VERIFIER" | openssl dgst -sha256 -binary | openssl base64 -A | tr '+/' '-_' | tr -d '=')
    out=$(curl -s -o /dev/null -w '%{http_code} %{redirect_url}' --get "$BASE/oauth/authorize" \
        --data-urlencode response_type=code --data-urlencode "client_id=$UI_CLIENT" \
        --data-urlencode "redirect_uri=$UI_BASE/callback" --data-urlencode "code_challenge=$UI_CHALLENGE" \
        --data-urlencode code_challenge_method=S256)
    UI_REQUEST=$(sed -E 's/.*request=(oareq_[0-9a-f]+).*/\1/' <<<"$out")
    step "the UI app's consent" 200 - -X POST "$BASE/admin/oauth/requests/$UI_REQUEST/approve"
    UI_CODE=$(sed -E 's/.*[?&]code=(hypha_ac_[0-9a-f]+).*/\1/' <<<"$BODY")
    oauth_step "the UI app's tokens" 200 "$(curl -s -w '\n%{http_code}' -X POST "$BASE/oauth/token" \
        --data-urlencode grant_type=authorization_code --data-urlencode "code=$UI_CODE" \
        --data-urlencode "code_verifier=$UI_VERIFIER" --data-urlencode "client_id=$UI_CLIENT" \
        --data-urlencode "redirect_uri=$UI_BASE/callback")"
    UI_TOKEN=$(sed -E 's/.*"access_token":"(hypha_at_[0-9a-f]+)".*/\1/' <<<"$BODY")

    step "what apps add to an admin" 200 - "$BASE/admin/app-extensions"
    contains "its page, absolute" "\"id\":\"dashboard\",\"title\":\"Dashboard\",\"url\":\"$UI_BASE/admin\""
    contains "its point action, to run elsewhere" '"id":"refund","label":"Refund","target":"payment","input_schema":null,"point":"payments.refund"'
    step "for one target" 200 - "$BASE/admin/app-extensions?target=order.payment"
    # No pages there, and its block (a build with spores lists theirs beside it).
    contains "no pages for it" '"pages":[],"blocks":['
    contains "its block" '{"app":"app_e2e_ui","id":"payment","target":"order.payment"'
    step "a target that isn't one" 400 invalid_query "$BASE/admin/app-extensions?target=order.paymnet"
    step "a session token for its page" 200 - -X POST "$BASE/admin/app-extensions/app_e2e_ui/session"
    contains "a JWT" '"token":"eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.'
    step "the admin's origins" 200 - "$BASE/admin/app-extensions/origins"
    E2E_PAY="pay_0190f1a2b3c47d8e9f00112233445566"
    E2E_ORD="ord_0190f1a2b3c47d8e9f00112233445566"
    step "run its action" 200 - -X POST "$BASE/admin/app-extensions/actions/app_e2e_ui/capture" -H "$JSON" \
        -d "{\"context\":{\"payment_id\":\"$E2E_PAY\"}}"
    contains "with its answer" '{"message":"Captured","refresh":true,"navigate":null}'
    step "an action without its context" 422 invalid_extension_context \
        -X POST "$BASE/admin/app-extensions/actions/app_e2e_ui/capture" -H "$JSON" -d '{}'
    step "an action that is a point" 409 app_action_has_point \
        -X POST "$BASE/admin/app-extensions/actions/app_e2e_ui/refund" -H "$JSON" \
        -d "{\"context\":{\"payment_id\":\"$E2E_PAY\"}}"
    step "fetch its block" 200 - "$BASE/admin/app-extensions/blocks/app_e2e_ui/payment?order_id=$E2E_ORD&payment_id=$E2E_PAY"
    contains "the checked tree" '"title":"Paid"'
    step "a block that isn't v1" 502 app_unavailable "$BASE/admin/app-extensions/blocks/app_e2e_ui/bad?order_id=$E2E_ORD"
    step "both calls are logged" 200 - "$BASE/admin/apps/app_e2e_ui/extension-calls"
    contains "the block's" '"point":"admin.block","version":1'
    contains "the action's" '"point":"admin.action","version":1'
    step "the admin proxy" 201 - -X POST "$BASE/admin/apps/app_e2e_ui/proxy/settings?x=1" -H "$JSON" -d '{"a":1}'
    contains "forwards the path and the body" '"method":"POST","path":"/admin-api/settings?x=1"'
    contains "with the context, signed" '"hypha-path":"/settings?x=1"'
    # A backslash, encoded: refused the same on every host. (A `..` segment is refused too,
    # tested in the route tests; workerd resolves `%2e%2e` before the Worker sees the URL,
    # so on Workers it never reaches the check, and the request stays inside the proxy.)
    step "a path with a backslash" 400 invalid_proxy_path "$BASE/admin/apps/app_e2e_ui/proxy/a%5cb"

    step "what apps add to a storefront" 200 - "$BASE/storefront/app-extensions"
    contains "its element, absolute" "\"tag\":\"e2e-pay\",\"module\":\"$UI_BASE/el.js\""
    step "a cart for the storefront proxy" 201 - -X POST "$BASE/storefront/carts" -H "$JSON" -d '{"country":"HR"}'
    UI_CART=$(sed -E 's/.*"id":"(cart_[0-9a-f]+)".*/\1/' <<<"$BODY")
    step "the storefront proxy" 200 - -H "x-hypha-cart: $UI_CART" "$BASE/storefront/apps/app_e2e_ui/proxy/reviews?page=2"
    contains "with the shopper's cart" "\"hypha-cart\":\"$UI_CART\""
    step "a cart header that isn't one" 400 invalid_cart_header -H "x-hypha-cart: nope" \
        "$BASE/storefront/apps/app_e2e_ui/proxy/reviews"

    write_ui_manifest '["orders:read","products:read","customers:read"]'
    step "a refresh asking for more" 200 - -X POST "$BASE/admin/apps/app_e2e_ui/refresh"
    contains "waits for consent" '"status":"consent_required"'
    write_ui_manifest '["orders:read"]'
    OWNER_AUTH=("${AUTH[@]}")
    AUTH=(-H "authorization: Bearer $UI_TOKEN")
    step "the app refreshes itself with fewer" 200 - -X POST "$BASE/admin/apps/app_e2e_ui/refresh"
    contains "which apply at once" '"grant":{"scopes":["orders:read"]'
    AUTH=("${OWNER_AUTH[@]}")
    step "uninstall the UI app" 204 - -X DELETE "$BASE/admin/apps/app_e2e_ui"
    step "an uninstalled app adds nothing" 409 app_not_installed -X POST "$BASE/admin/app-extensions/app_e2e_ui/session"
    step "delete the UI app" 204 - -X DELETE "$BASE/admin/apps/app_e2e_ui"
else
    echo "skip app UI steps: no python3 for the app's site"
fi

echo
echo "## private apps (design note 0029, §4.1)"
PRIVATE_MANIFEST='{"key":"app_e2e_private","name":"E2E private","version":"1.0.0",
 "app_url":"https://rates.e2e.example","redirect_uris":[],
 "scopes":["shipping:read","customer_data:address"],
 "scope_reasons":{"customer_data:address":"quotes by postal code"},"privacy_url":"/privacy"}'
step "an upload is checked like a manifest" 422 manifest_invalid -X POST "$BASE/admin/apps/private" -H "$JSON" \
    -d '{"manifest":{"key":"app_e2e_private","name":"E2E private","version":"1.0.0","app_url":"ftp://x","redirect_uris":[],"privacy_url":"/p"}}'
step "upload a private app's manifest" 201 - -X POST "$BASE/admin/apps/private" -H "$JSON" \
    -d "{\"manifest\":$PRIVATE_MANIFEST}"
contains "under its URN" '"client_id":"urn:hypha:private:app_e2e_private"'
PRIVATE_REQUEST=$(sed -E 's/.*"id":"(oareq_[0-9a-f]+)".*/\1/' <<<"$BODY")
step "approving it answers with its credentials" 200 - -X POST "$BASE/admin/oauth/requests/$PRIVATE_REQUEST/approve"
contains "and no redirect" '"redirect_to":null'
contains "its secret, once" '"app_secret":"hypha_as_'
PRIVATE_TOKEN=$(sed -E 's/.*"access_token":"(hypha_at_[0-9a-f]+)".*/\1/' <<<"$BODY")
OWNER_AUTH=("${AUTH[@]}")
AUTH=(-H "authorization: Bearer $PRIVATE_TOKEN")
step "its token reads shipping" 200 - "$BASE/admin/shipping-zones"
AUTH=("${OWNER_AUTH[@]}")
step "a private app isn't refreshed" 409 app_is_private -X POST "$BASE/admin/apps/app_e2e_private/refresh"
step "uninstall the private app" 204 - -X DELETE "$BASE/admin/apps/app_e2e_private"
step "delete the private app" 204 - -X DELETE "$BASE/admin/apps/app_e2e_private"

echo
echo "## errors"
step "unknown route" 404 route_not_found "$BASE/nope"
step "wrong method" 405 method_not_allowed -X DELETE "$BASE/admin/products/$MUG"
step "malformed json" 400 malformed_json -X POST "$BASE/admin/markets" -H "$JSON" -d '{"name":'
step "missing field" 422 invalid_body -X POST "$BASE/admin/markets" -H "$JSON" -d '{"name":"X"}'
step "withdrawn currency" 422 invalid_body -X POST "$BASE/admin/markets" -H "$JSON" \
    -d '{"name":"Croatia 2022","countries":["HR"],"currency":"HRK"}'
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
step "cart to an unassigned country" 422 unknown_country_code -X POST "$BASE/storefront/carts" -H "$JSON" \
    -d '{"country":"XK"}'
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
    -d "{\"kind\":\"publishable\",\"name\":\"Storefront\",\"channel\":\"$ONLINE_STORE\"}"
PUBLISHABLE=$(token)
SAVED_SF_AUTH=("${SF_AUTH[@]}")
SF_AUTH=(-H "x-publishable-key: $PUBLISHABLE")
step "a storefront reads the store" 200 - "$BASE/storefront/store"
contains "its public details" '"description":"Mugs and cups.","contact_email":"hello@e2e.example","default_language":"en"'
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
INVITE=$(id)
INVITE_TOKEN=$(mail_token support@example.com "invited" hypha_inv_)
mail "the invite arrived" support@example.com "invited"
step "staff can't be invited" 409 user_exists -X POST "$BASE/admin/invites" -H "$JSON" \
    -d "{\"email\":\"$OWNER_EMAIL\",\"role\":\"$ROLE\"}"
step "pending invites (design note 0024)" 200 - "$BASE/admin/invites"
contains "lists the invite" '"email":"support@example.com"'
SAVED_AUTH=("${AUTH[@]}")
AUTH=()
if [[ -n ${INVITE_TOKEN:-} ]]; then
    step "the invite's link shows what it is" 200 - -X POST "$BASE/admin/invites/lookup" \
        -H "$JSON" -d "{\"token\":\"$INVITE_TOKEN\"}"
    contains "its email, role and sender" '"email":"support@example.com","role":"Support","invited_by":"Owner"'
fi
step "a made-up token looks up nothing" 401 invalid_token -X POST "$BASE/admin/invites/lookup" \
    -H "$JSON" -d '{"token":"hypha_inv_nope"}'
AUTH=("${SAVED_AUTH[@]}")
step "revoke the invite" 204 - -X DELETE "$BASE/admin/invites/$INVITE"
step "revoking it again" 404 invite_not_found -X DELETE "$BASE/admin/invites/$INVITE"
step "no invites pending" 200 - "$BASE/admin/invites"
contains "the list is empty" '"items":[]'
if [[ -n ${INVITE_TOKEN:-} ]]; then
    AUTH=()
    step "a revoked invite's link stops working" 401 invalid_token -X POST \
        "$BASE/admin/invites/accept" -H "$JSON" \
        -d "{\"token\":\"$INVITE_TOKEN\",\"name\":\"Sam\",\"password\":\"sam password\"}"
    AUTH=("${SAVED_AUTH[@]}")
fi
step "rename myself, no users:write needed" 200 - -X PATCH "$BASE/admin/me" -H "$JSON" \
    -d '{"name":"Owner Two"}'
contains "the new name" '"name":"Owner Two"'
step "and back" 200 - -X PATCH "$BASE/admin/me" -H "$JSON" -d '{"name":"Owner"}'
step "sign in on a second device" 200 - -X POST "$BASE/admin/auth/login" -H "$JSON" \
    -H "user-agent: E2E second device" \
    -d "{\"email\":\"$OWNER_EMAIL\",\"password\":\"$OWNER_PASSWORD\"}"
SECOND=$(token)
step "my sessions" 200 - "$BASE/admin/me/sessions"
contains "this one is marked" '"current":true'
contains "the second device is listed" '"user_agent":"E2E second device"'
SECOND_ID=$(sed -E 's/^[^"]*"items":\[\{"id":"(sess_[0-9a-f]+)".*/\1/' <<<"$BODY")
step "sign the second device out" 204 - -X DELETE "$BASE/admin/me/sessions/$SECOND_ID"
step "it's signed out already" 404 session_not_found -X DELETE "$BASE/admin/me/sessions/$SECOND_ID"
AUTH=(-H "authorization: Bearer $SECOND")
step "the second device's session is over" 401 invalid_token "$BASE/admin/me"
AUTH=("${SAVED_AUTH[@]}")
step "sign out everywhere else" 204 - -X POST "$BASE/admin/me/sessions/revoke-others"
step "this session still works" 200 - "$BASE/admin/me"
step "every role" 200 - "$BASE/admin/roles"
step "every staff user" 200 - "$BASE/admin/users"
contains "the owner is staff" "\"email\":\"$OWNER_EMAIL\""
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
