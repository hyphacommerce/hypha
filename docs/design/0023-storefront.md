# 0023: The storefront app

- **Status:** Accepted (2026-10-17). The outline in 0021 §9, made concrete: Hypha's own
  storefront, the first client of the storefront API that a shopper uses.
- **Scope:**
  - `apps/storefront`, a server-rendered shop on the admin's stack;
  - routing by country (0021 §6), with folders by default;
  - the pages: home, category, product, search, cart and checkout, the order's
    confirmation;
  - where the publishable key and the cart live.
- **Not in scope:**
  - payments (0021, SU8: orders are placed unpaid);
  - customer accounts (0008 §9);
  - translations (0021 §5 leaves room);
  - a content system for pages, banners and the store's logo (0021, Q3);
  - a theme system for merchants; the app is a starting point to fork.
- **Related:** 0010 (API surfaces), 0011 (the admin's stack), 0019 (search), 0020
  (channels), 0021 (store setup), 0022 (categories).

## Decisions

| | Decision |
| --- | --- |
| SF1 | **The admin's stack.** Next.js App Router on vinext and Vite, server-rendered, Cloudflare Workers in development as in production. Only standard Next.js APIs, so `next build` keeps working too. *Decided* |
| SF2 | **The publishable key stays on the server.** It isn't a secret (0008), but nothing in the browser needs it: pages render on the server and changes go through server actions. `STOREFRONT_KEY` and `HYPHA_API_URL` live in `.dev.vars` (a Worker secret in production). *Decided* |
| SF3 | **One folder per country by default** (`/hr/…`, lowercase). `proxy.ts` sends a path without one to the shopper's country: the `country` cookie, else the host's geolocation header (`x-vercel-ip-country`, `cf-ipcountry`), else the store's first country. *Decided* |
| SF4 | **One tree as a setting.** `STOREFRONT_ROUTING=single` keeps URLs without a country: `proxy.ts` rewrites each request to the country folder internally, from the same cookie and headers. The pages are the same in both modes. *Decided* |
| SF5 | **The cart ID in an httpOnly cookie**, one per country (a cart's country decides its market and currency). Every change is a server action, and the page re-renders from the API's answer. *Decided* |
| SF6 | **Money with the country's minor units.** `/storefront/countries` gives each country's currency and minor units; amounts are never assumed to have two decimals. *Decided* |
| SF7 | **Listings are searches.** Home's new products, a category page and the search page all read `/storefront/search` (0019), with `category` for a category (0022), so facets, sorting and paging work the same everywhere. *Decided* |
| SF8 | **Plain CSS, no component library.** A shopper-facing site is mostly layout and type; CSS modules keep it small and easy to restyle. *Decided* |

## 1. Configuration

| Variable | Meaning |
| --- | --- |
| `HYPHA_API_URL` | Where the API runs (server-side only). |
| `STOREFRONT_KEY` | A publishable key (`hypha_pk_…`) for the channel this storefront sells through (0020). |
| `STOREFRONT_ROUTING` | `folders` (the default) or `single`. |

The dev server runs on port 3002 (`pnpm storefront:dev`).

## 2. Routing (SF3, SF4)

- **Country folders:**
  - every page lives under `app/[country]/`;
  - the folder's layout checks the country against `/storefront/countries` and answers
    404 for one no market sells to;
  - links are built by one helper, `href(country, path)`, which leaves the folder out in
    `single` mode.
- **`proxy.ts`:**
  - **folders:** a path whose first segment isn't a country the store sells to is
    redirected to `/{country}{path}`;
  - **single:** every page request is rewritten to `/{country}{path}`, so the URL keeps
    no country;
  - the country comes from the `country` cookie, then the geolocation header, then the
    store's first country;
  - the store's countries are cached in the proxy for a minute; they change rarely.
- **The country picker** (in the header) sets the cookie and goes to the same page in
  the new country. A product page in a country that doesn't sell the product says so
  (0021, Q2).

## 3. Pages

- **Home:**
  - the store's name and description;
  - its top categories;
  - its newest products (search, `sort=newest`).
- **Category** (`/{country}/c/{handle}`):
  - the category with its breadcrumb and subcategories;
  - its products through search with `category`, including subcategories (0022, CA3);
  - facets as filters, sorting, and paging by cursor.
- **Product** (`/{country}/p/{handle}`):
  - the product read with prices for the country (0021, SU7);
  - option pickers choose the variant, and its price, availability and image follow;
  - "Add to cart" adds it;
  - "Not available in your country" when `sold_in_country` is false.
- **Search** (`/{country}/search?q=…`): the same listing as a category, by words.
- **Cart** (`/{country}/cart`):
  - lines with quantities;
  - unavailable lines say why and leave the totals;
  - totals with tax as the market shows it.
- **Checkout** (`/{country}/checkout`), three steps on one page:
  - email;
  - shipping address (in the cart's country);
  - a shipping option per group (0015).

  "Place order" checks out with the total shown (`expected_total`), so a price that moved
  is caught.
- **Confirmation** (`/{country}/order`):
  - read through the cart, which names its order once checked out;
  - the cart cookie is then cleared.

## 4. Later

- **Payments:** a payment step comes with the spore API. Checkout places orders unpaid
  until then.
- **Customer accounts and order history.**
- **Translations:** reads will take a `language` (0021, SU5).
- **A content system** for pages, the logo and banners.
- **Images resized per breakpoint** (`srcset` from the media URLs).
