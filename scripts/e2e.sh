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
# The first address id of the last customer response.
address() { sed -E 's/.*"addresses":\[\{"id":"(addr_[0-9a-f]+)".*/\1/' <<<"$BODY"; }
# The first variant's id of the last product response.
variant() { sed -E 's/.*"variants":\[[^]]*"id":"(var_[0-9a-f]+)".*/\1/' <<<"$BODY"; }

if ! curl -s -o /dev/null "$BASE/"; then
    echo "No server at $BASE. Start it with: cargo run -p hypha-host-axum"
    exit 2
fi

echo "## happy path"
step "the store this deployment serves" 200 - "$BASE/store"
contains "store has a store_ ID" '"id":"store_'
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
contains "prices exclude tax by default" '"prices_include_tax":false'
step "prices include tax" 200 - -X PATCH "$BASE/markets/$MARKET" -H "$JSON" -d '{"prices_include_tax":true}'
contains "market now includes tax" '"prices_include_tax":true'
step "back to excluding tax" 200 - -X PATCH "$BASE/markets/$MARKET" -H "$JSON" -d '{"prices_include_tax":false}'
step "sell to Slovenia too" 200 - -X POST "$BASE/markets/$MARKET/countries" -H "$JSON" -d '{"country":"SI"}'
contains "Slovenia added" '"countries":["HR","DE","SI"]'
step "stop selling to Slovenia" 200 - -X DELETE "$BASE/markets/$MARKET/countries/SI"
contains "Slovenia removed" '"countries":["HR","DE"]'
step "create price list (-10%)" 201 - -X POST "$BASE/price-lists" -H "$JSON" \
    -d '{"currency":"EUR","adjustment_bps":-1000}'
LIST=$(id)
step "fix cup price at 200" 204 - -X PUT "$BASE/price-lists/$LIST/fixed-prices" -H "$JSON" \
    -d "{\"fixed_prices\":[{\"variant\":\"$CUP_VAR\",\"price\":{\"amount\":200,\"currency\":\"EUR\"}}]}"
step "create catalog" 201 - -X POST "$BASE/catalogs" -H "$JSON" -d "{\"market\":\"$MARKET\"}"
CATALOG=$(id)
step "add mug and cup to catalog" 204 - -X POST "$BASE/catalogs/$CATALOG/products" -H "$JSON" \
    -d "{\"products\":[\"$MUG\",\"$CUP\"]}"
step "assign price list" 200 - -X PUT "$BASE/catalogs/$CATALOG/price-list" -H "$JSON" \
    -d "{\"price_list\":\"$LIST\"}"

step "create cart" 201 - -X POST "$BASE/carts" -H "$JSON" -d '{"country":"HR"}'
contains "cart ships to Croatia" '"country":"HR"'
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

step "remove cup's fixed price" 204 - -X DELETE "$BASE/price-lists/$LIST/fixed-prices" -H "$JSON" \
    -d "{\"variants\":[\"$CUP_VAR\"]}"
step "get cart" 200 - "$BASE/carts/$CART"
contains "cup falls back to 250 - 10% = 225" '"unit_price":{"amount":225,"currency":"EUR"},"total":{"amount":675'
step "remove cup from catalog" 204 - -X DELETE "$BASE/catalogs/$CATALOG/products" -H "$JSON" \
    -d "{\"products\":[\"$CUP\"]}"
step "get cart" 200 - "$BASE/carts/$CART"
contains "cup line is unavailable" '"status":"unavailable","reason_code":"not_available_in_market"'

echo "## tax"
step "create book" 201 - -X POST "$BASE/products" -H "$JSON" \
    -d '{"handle":"book","title":"Book","price":{"amount":1000,"currency":"EUR"}}'
BOOK=$(id)
BOOK_VAR=$(variant)
step "activate book" 200 - -X PATCH "$BASE/products/$BOOK/status" -H "$JSON" -d '{"status":"active"}'
step "add book to catalog" 204 - -X POST "$BASE/catalogs/$CATALOG/products" -H "$JSON" \
    -d "{\"products\":[\"$BOOK\"]}"
step "create tax class" 201 - -X POST "$BASE/tax-classes" -H "$JSON" -d '{"name":"Books"}'
BOOKS=$(id)
step "put book in it" 204 - -X PUT "$BASE/tax-assignments" -H "$JSON" \
    -d "{\"products\":[\"$BOOK\"],\"tax_class\":\"$BOOKS\"}"
step "create Croatian tax region (25%)" 201 - -X POST "$BASE/tax-regions" -H "$JSON" \
    -d '{"country":"HR","default_rate_ppm":250000}'
REGION=$(id)
step "books pay 5% in Croatia" 200 - -X PUT "$BASE/tax-regions/$REGION/class-rates/$BOOKS" \
    -H "$JSON" -d '{"rate_ppm":50000}'
contains "class rate is stored" "\"class_rates\":[{\"tax_class\":\"$BOOKS\",\"rate_ppm\":50000}]"
step "get tax region" 200 - "$BASE/tax-regions/$REGION"
step "create taxed cart" 201 - -X POST "$BASE/carts" -H "$JSON" -d '{"country":"HR"}'
TAX_CART=$(id)
step "add 2 books" 200 - -X POST "$BASE/carts/$TAX_CART/lines" -H "$JSON" \
    -d "{\"variant\":\"$BOOK_VAR\",\"quantity\":2}"
contains "book line pays 5% of 1800" "\"tax\":{\"rate_ppm\":50000,\"amount\":{\"amount\":90,\"currency\":\"EUR\"},\"tax_class\":\"$BOOKS\"}"
contains "tax is added on top" '"taxes_included":false,"subtotal":{"amount":1800,"currency":"EUR"},"tax_total":{"amount":90,"currency":"EUR"},"total":{"amount":1890'
step "prices include tax" 200 - -X PATCH "$BASE/markets/$MARKET" -H "$JSON" -d '{"prices_include_tax":true}'
step "get taxed cart" 200 - "$BASE/carts/$TAX_CART"
contains "tax is extracted (1800 x 5/105 = 85.7)" '"taxes_included":true,"subtotal":{"amount":1800,"currency":"EUR"},"tax_total":{"amount":86,"currency":"EUR"},"total":{"amount":1800'
step "remove books rate" 200 - -X DELETE "$BASE/tax-regions/$REGION/class-rates/$BOOKS"
contains "no class rates left" '"class_rates":[]'
step "get taxed cart" 200 - "$BASE/carts/$TAX_CART"
contains "book pays the 25% default (1800 x 25/125)" '"tax_total":{"amount":360,"currency":"EUR"}'
step "lower the default to 13%" 200 - -X PATCH "$BASE/tax-regions/$REGION" -H "$JSON" -d '{"default_rate_ppm":130000}'
step "get taxed cart" 200 - "$BASE/carts/$TAX_CART"
contains "book pays 13% (1800 x 13/113 = 207.1)" '"tax_total":{"amount":207,"currency":"EUR"}'
step "back to excluding tax" 200 - -X PATCH "$BASE/markets/$MARKET" -H "$JSON" -d '{"prices_include_tax":false}'

echo "## inventory"
step "create lamp" 201 - -X POST "$BASE/products" -H "$JSON" \
    -d '{"handle":"lamp","title":"Lamp","price":{"amount":1000,"currency":"EUR"}}'
LAMP=$(id)
LAMP_VAR=$(variant)
step "activate lamp" 200 - -X PATCH "$BASE/products/$LAMP/status" -H "$JSON" -d '{"status":"active"}'
step "add lamp to catalog" 204 - -X POST "$BASE/catalogs/$CATALOG/products" -H "$JSON" \
    -d "{\"products\":[\"$LAMP\"]}"
step "lamp has no stock set up" 200 - "$BASE/variants/$LAMP_VAR/inventory"
contains "so it is untracked" '"id":null,'
contains "reported as tracked: false" '"tracked":false'
step "create warehouse for the market" 201 - -X POST "$BASE/locations" -H "$JSON" \
    -d "{\"name\":\"Zagreb\",\"markets\":[\"$MARKET\"]}"
WAREHOUSE=$(id)
step "create lamp inventory" 201 - -X POST "$BASE/inventory-items" -H "$JSON" \
    -d "{\"variant\":\"$LAMP_VAR\"}"
LAMP_ITEM=$(id)
contains "tracked by default" '"tracked":true,"continue_selling":false,"levels":[]'
step "2 lamps on hand" 200 - -X PUT "$BASE/inventory-items/$LAMP_ITEM/levels/$WAREHOUSE" -H "$JSON" \
    -d '{"on_hand":2}'
contains "level is stored" "\"levels\":[{\"location\":\"$WAREHOUSE\",\"on_hand\":2,\"reserved\":0,\"available\":2}]"
step "create stock cart" 201 - -X POST "$BASE/carts" -H "$JSON" -d '{"country":"HR"}'
STOCK_CART=$(id)
step "add 3 lamps" 409 out_of_stock -X POST "$BASE/carts/$STOCK_CART/lines" -H "$JSON" \
    -d "{\"variant\":\"$LAMP_VAR\",\"quantity\":3}"
step "add 2 lamps" 200 - -X POST "$BASE/carts/$STOCK_CART/lines" -H "$JSON" \
    -d "{\"variant\":\"$LAMP_VAR\",\"quantity\":2}"
contains "lamps are available" '"fully_available":true'
step "count with a stale expected" 409 on_hand_changed -X PUT \
    "$BASE/inventory-items/$LAMP_ITEM/levels/$WAREHOUSE" -H "$JSON" -d '{"on_hand":1,"expected_on_hand":5}'
step "a lamp broke (1 left)" 200 - -X PUT "$BASE/inventory-items/$LAMP_ITEM/levels/$WAREHOUSE" \
    -H "$JSON" -d '{"on_hand":1,"expected_on_hand":2}'
step "get stock cart" 200 - "$BASE/carts/$STOCK_CART"
contains "lamp line ran out" '"status":"unavailable","reason_code":"out_of_stock"'
contains "and is left out of the total" '"total":{"amount":0,"currency":"EUR"},"fully_available":false'
step "delivery of 4" 200 - -X POST "$BASE/inventory-items/$LAMP_ITEM/levels/$WAREHOUSE/adjustments" \
    -H "$JSON" -d '{"delta":4}'
contains "5 on hand" '"on_hand":5'
step "get stock cart" 200 - "$BASE/carts/$STOCK_CART"
contains "lamps are back" '"fully_available":true'
step "raise lamps beyond stock" 409 out_of_stock -X PUT "$BASE/carts/$STOCK_CART/lines/$LAMP_VAR" \
    -H "$JSON" -d '{"quantity":6}'
step "set lamps to 3" 200 - -X PUT "$BASE/carts/$STOCK_CART/lines/$LAMP_VAR" -H "$JSON" -d '{"quantity":3}'
contains "line holds 3" '"quantity":3'
step "stop tracking lamps" 200 - -X PATCH "$BASE/inventory-items/$LAMP_ITEM" -H "$JSON" -d '{"tracked":false}'
step "add 100 untracked lamps" 200 - -X POST "$BASE/carts/$STOCK_CART/lines" -H "$JSON" \
    -d "{\"variant\":\"$LAMP_VAR\",\"quantity\":100}"
step "remove the lamp line" 200 - -X DELETE "$BASE/carts/$STOCK_CART/lines/$LAMP_VAR"
contains "cart is empty again" '"lines":[]'
step "remove it again" 404 line_not_found -X DELETE "$BASE/carts/$STOCK_CART/lines/$LAMP_VAR"
step "lower warehouse priority" 200 - -X PATCH "$BASE/locations/$WAREHOUSE" -H "$JSON" -d '{"priority":5}'
contains "priority is stored" '"priority":5'
step "get warehouse" 200 - "$BASE/locations/$WAREHOUSE"

echo "## customers"
step "create customer" 201 - -X POST "$BASE/customers" -H "$JSON" \
    -d '{"email":" Ana@Example.HR ","first_name":"Ana"}'
CUSTOMER=$(id)
contains "email is normalized" '"email":"ana@example.hr","first_name":"Ana"'
step "rename customer" 200 - -X PATCH "$BASE/customers/$CUSTOMER" -H "$JSON" \
    -d '{"first_name":null,"last_name":"Horvat"}'
contains "null clears, others change" '"first_name":null,"last_name":"Horvat"'
step "add shipping address" 201 - -X POST "$BASE/customers/$CUSTOMER/addresses" -H "$JSON" \
    -d '{"last_name":"Horvat","line1":"Ilica 1","city":"Zagreb","postal_code":"10000","country":"HR","default_shipping":true}'
ADDRESS=$(address)
contains "it is the default shipping address" "\"default_shipping_address\":\"$ADDRESS\",\"default_billing_address\":null"
step "move it to Split, billing only" 200 - -X PUT "$BASE/customers/$CUSTOMER/addresses/$ADDRESS" -H "$JSON" \
    -d '{"last_name":"Horvat","line1":"Riva 2","city":"Split","country":"HR","default_billing":true}'
contains "defaults follow the request" "\"default_shipping_address\":null,\"default_billing_address\":\"$ADDRESS\""
contains "address is replaced" '"city":"Split"'
step "get customer" 200 - "$BASE/customers/$CUSTOMER"
step "create buyer cart" 201 - -X POST "$BASE/carts" -H "$JSON" -d '{"country":"HR"}'
BUYER_CART=$(id)
contains "no buyer yet" '"email":null,"customer":null'
step "set cart email" 200 - -X PUT "$BASE/carts/$BUYER_CART/email" -H "$JSON" -d '{"email":"Ivo@Example.hr"}'
contains "cart email is normalized" '"email":"ivo@example.hr","customer":null'
step "link the customer" 200 - -X PUT "$BASE/carts/$BUYER_CART/customer" -H "$JSON" \
    -d "{\"customer\":\"$CUSTOMER\"}"
contains "cart takes the customer's email" "\"email\":\"ana@example.hr\",\"customer\":\"$CUSTOMER\""
step "unlink the customer" 200 - -X PUT "$BASE/carts/$BUYER_CART/customer" -H "$JSON" -d '{"customer":null}'
contains "email stays" '"email":"ana@example.hr","customer":null'
step "remove the address" 200 - -X DELETE "$BASE/customers/$CUSTOMER/addresses/$ADDRESS"
contains "address book is empty, defaults cleared" '"default_billing_address":null,"addresses":[]'

echo "## errors"
step "unknown route" 404 route_not_found "$BASE/nope"
step "wrong method" 405 method_not_allowed -X DELETE "$BASE/products/$MUG"
step "malformed json" 400 malformed_json -X POST "$BASE/markets" -H "$JSON" -d '{"name":'
step "missing field" 422 invalid_body -X POST "$BASE/markets" -H "$JSON" -d '{"name":"X"}'
step "unknown currency" 422 invalid_body -X POST "$BASE/markets" -H "$JSON" \
    -d '{"name":"UK","countries":["GB"],"currency":"GBP"}'
step "amount beyond 2^53-1" 422 invalid_body -X POST "$BASE/products" -H "$JSON" \
    -d '{"handle":"huge","title":"Huge","price":{"amount":9007199254740992,"currency":"EUR"}}'
step "wrong id prefix" 400 invalid_path "$BASE/carts/$MUG"
step "unknown cart" 404 cart_not_found "$BASE/carts/cart_00000000000000000000000000000063"
step "duplicate handle" 409 handle_taken -X POST "$BASE/products" -H "$JSON" \
    -d '{"handle":"mug","title":"Mug 2","price":{"amount":1,"currency":"EUR"}}'
step "country in other market" 409 country_in_other_market -X POST "$BASE/markets" -H "$JSON" \
    -d '{"name":"DACH","countries":["AT","DE"],"currency":"EUR"}'
step "create Serbia market" 201 - -X POST "$BASE/markets" -H "$JSON" \
    -d '{"name":"Serbia","countries":["RS"],"currency":"RSD"}'
SERBIA=$(id)
step "add a country another market has" 409 country_in_other_market -X POST \
    "$BASE/markets/$MARKET/countries" -H "$JSON" -d '{"country":"RS"}'
step "remove a market's last country" 422 last_country -X DELETE "$BASE/markets/$SERBIA/countries/RS"
step "remove a country the market lacks" 404 country_not_in_market -X DELETE \
    "$BASE/markets/$MARKET/countries/FR"
step "second catalog for market" 409 catalog_exists -X POST "$BASE/catalogs" -H "$JSON" \
    -d "{\"market\":\"$MARKET\"}"
step "create RSD price list" 201 - -X POST "$BASE/price-lists" -H "$JSON" -d '{"currency":"RSD"}'
RSD_LIST=$(id)
step "assign RSD list to EUR market" 422 price_list_currency_mismatch -X PUT \
    "$BASE/catalogs/$CATALOG/price-list" -H "$JSON" -d "{\"price_list\":\"$RSD_LIST\"}"
step "fixed prices on unknown list" 404 price_list_not_found -X PUT \
    "$BASE/price-lists/pl_00000000000000000000000000000063/fixed-prices" -H "$JSON" -d '{"fixed_prices":[]}'
step "fixed price in other currency" 422 fixed_price_currency_mismatch -X PUT \
    "$BASE/price-lists/$LIST/fixed-prices" -H "$JSON" \
    -d "{\"fixed_prices\":[{\"variant\":\"$CUP_VAR\",\"price\":{\"amount\":200,\"currency\":\"RSD\"}}]}"
step "misspelled PATCH field" 422 invalid_body -X PATCH "$BASE/customers/$CUSTOMER" -H "$JSON" \
    -d '{"frist_name":"Ana"}'
step "null for a field that can't be cleared" 422 invalid_body -X PATCH \
    "$BASE/customers/$CUSTOMER" -H "$JSON" -d '{"email":null}'
step "empty PATCH changes nothing" 200 - -X PATCH "$BASE/markets/$MARKET" -H "$JSON" -d '{}'
step "update unknown market" 404 market_not_found -X PATCH \
    "$BASE/markets/mkt_00000000000000000000000000000063" -H "$JSON" -d '{"prices_include_tax":true}'
step "products for unknown catalog" 404 catalog_not_found -X POST \
    "$BASE/catalogs/cat_00000000000000000000000000000063/products" -H "$JSON" -d '{"products":[]}'
step "add archived mug" 422 product_not_active -X POST "$BASE/carts/$CART/lines" -H "$JSON" \
    -d "{\"variant\":\"$MUG_VAR\",\"quantity\":1}"
step "cart to a country without market" 422 no_market_for_country -X POST "$BASE/carts" -H "$JSON" \
    -d '{"country":"US"}'
step "cart to a lowercase country" 422 invalid_country_code -X POST "$BASE/carts" -H "$JSON" \
    -d '{"country":"us"}'
step "second tax region for a country" 409 tax_region_exists -X POST "$BASE/tax-regions" -H "$JSON" \
    -d '{"country":"HR","default_rate_ppm":250000}'
step "tax rate above 100%" 422 invalid_tax_rate -X POST "$BASE/tax-regions" -H "$JSON" \
    -d '{"country":"DE","default_rate_ppm":1000001}'
step "unknown tax region" 404 tax_region_not_found "$BASE/tax-regions/txreg_00000000000000000000000000000063"
step "rate for unknown class" 404 tax_class_not_found -X PUT \
    "$BASE/tax-regions/$REGION/class-rates/txc_00000000000000000000000000000063" -H "$JSON" -d '{"rate_ppm":0}'
step "remove a rate the class doesn't have" 404 class_rate_not_found -X DELETE \
    "$BASE/tax-regions/$REGION/class-rates/$BOOKS"
step "assign unknown class" 422 tax_class_not_found -X PUT "$BASE/tax-assignments" -H "$JSON" \
    -d "{\"products\":[\"$BOOK\"],\"tax_class\":\"txc_00000000000000000000000000000063\"}"
step "second item for a variant" 409 inventory_item_exists -X POST "$BASE/inventory-items" -H "$JSON" \
    -d "{\"variant\":\"$LAMP_VAR\"}"
step "unknown inventory item" 404 inventory_item_not_found \
    "$BASE/inventory-items/invi_00000000000000000000000000000063"
step "stock at unknown location" 404 location_not_found -X PUT \
    "$BASE/inventory-items/$LAMP_ITEM/levels/loc_00000000000000000000000000000063" -H "$JSON" -d '{"on_hand":1}'
step "unknown location" 404 location_not_found "$BASE/locations/loc_00000000000000000000000000000063"
step "location listing a market twice" 422 duplicate_market -X POST "$BASE/locations" -H "$JSON" \
    -d "{\"name\":\"Split\",\"markets\":[\"$MARKET\",\"$MARKET\"]}"
step "blank location name" 422 field_required -X POST "$BASE/locations" -H "$JSON" -d '{"name":" "}'
step "stock beyond 2^53-1" 422 stock_out_of_range -X PUT \
    "$BASE/inventory-items/$LAMP_ITEM/levels/$WAREHOUSE" -H "$JSON" -d '{"on_hand":9007199254740992}'
step "same email in another case" 409 customer_exists -X POST "$BASE/customers" -H "$JSON" \
    -d '{"email":"ANA@example.hr"}'
step "invalid customer email" 422 invalid_email -X POST "$BASE/customers" -H "$JSON" -d '{"email":"ana"}'
step "address without a city" 422 invalid_address -X POST "$BASE/customers/$CUSTOMER/addresses" \
    -H "$JSON" -d '{"last_name":"Horvat","line1":"Ilica 1","city":" ","country":"HR"}'
step "unknown customer" 404 customer_not_found "$BASE/customers/cus_00000000000000000000000000000063"
step "remove unknown address" 404 address_not_found -X DELETE \
    "$BASE/customers/$CUSTOMER/addresses/addr_00000000000000000000000000000063"
step "link unknown customer to cart" 422 customer_not_found -X PUT "$BASE/carts/$BUYER_CART/customer" \
    -H "$JSON" -d '{"customer":"cus_00000000000000000000000000000063"}'
step "invalid cart email" 422 invalid_email -X PUT "$BASE/carts/$BUYER_CART/email" -H "$JSON" \
    -d '{"email":"nope"}'
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
