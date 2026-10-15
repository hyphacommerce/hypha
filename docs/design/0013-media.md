# 0013: Media, file storage and delivery

- **Status:** Accepted (2026-10-15); building phase 1 (section 10). Danilo set the direction in conversation: media is a
  first-class part of Hypha, like Shopify's; MinIO in Docker for local development, so S3
  incompatibilities show up early; every asset knows where it is used; proper image
  metadata; a CDN story for every host; others can bring their own DAM. Answers given
  on 2026-10-15 are marked *Decided*.
- **Scope:** the `Asset` aggregate and the `hypha-media` crate, uploads, storage behind
  the S3 API, delivery URLs and image sizes (the CDN), private files, attaching media to
  products and variants, usage tracking, external DAMs, the admin media library, local
  development.
- **Not in scope:** video transcoding and streaming (videos are stored and served as
  files), 3D models, delivering digital products to buyers (it reuses private assets
  later), media on collections and other future owners (same model, section 6), AI alt
  text (an admin assistant feature, 0002).
- **Related:** 0002 (spores, the outbox), 0006 (one store per deployment), 0010 (API
  surfaces), 0011 (the admin), 0012 (lists).

## Decisions

| | Decision |
| --- | --- |
| M1 | **The media model is core; storage, delivery and sources are ports.** A leaf crate `hypha-media` owns assets and their usages. Where the bytes live (`BlobStore`), how URLs and sizes are built (`MediaUrls`) and where assets come from (uploads, or an external DAM) are ports with adapters. Products, the storefront, search and the admin rely on one shape; nobody is stuck with our storage, CDN or library (section 1) |
| M2 | **An `Asset` (`asset_…`) is one file plus its metadata**: kind (image, video, file), MIME type, size, checksum, width and height, alt text, title, visibility, source and status (section 2). *Hard to reverse* (the API shape) |
| M3 | **Uploads go straight from the browser to storage** through presigned URLs: create the asset (pending) → upload → complete. Completing checks the stored object and reads its real type and dimensions from the file itself. The API never carries file bytes (section 3) |
| M4 | **Every file version has its own immutable key and URL**: `assets/{asset_id}/{version}/{filename}`. Replacing a file makes a new version, so the CDN can cache forever and nothing needs purging (section 4). *Hard to reverse* (URLs get published) |
| M5 | **Delivery is a `MediaUrls` port**: one public base URL setting (`HYPHA_MEDIA_URL`), and image sizes as URL transforms (Cloudflare Image Transformations, imgproxy, or none). The API returns ready URLs; storefronts ask for sizes without knowing the host (section 4) |
| M6 | **Private assets never get a public URL**: they are served through short-lived signed URLs, for digital goods and documents later (section 4) |
| M7 | **Products own an ordered media list; a variant points at one item of it.** The first item is the featured image. Products store asset IDs unchecked, like other cross-domain IDs; the attach commands live in a new composing crate, `hypha-merchandising` (section 5). *Hard to reverse*; *Decided* |
| M8 | **Usage is recorded, not derived**: each attachment writes an `AssetUsage` (asset, owner, role) in the same batch as the owner's write. "Used by" is one query for any owner, including spores, and an asset in use can't be deleted (section 6) |
| M9 | **External DAMs plug in as asset sources.** A spore creates Hypha assets that point at its own files and URLs and keeps their metadata in sync; products attach them like uploads (section 7) |
| M10 | **S3 API for storage**: Cloudflare R2, AWS S3 and MinIO with one adapter. **MinIO in `compose.yaml` for local development** and for the `BlobStore` contract, like Postgres for the repositories (section 8) |
| M11 | **Alt text and title live on the asset**, so they are written once and follow it everywhere. A per-usage alt override (the same photo described differently on two products) can be added to `AssetUsage` later without breaking anything. *Decided* |
| M12 | **No SVG uploads until we can sanitise them.** SVG files can carry scripts; refusing them is the safe default, and supporting them later is additive. *Decided* |

## 1. Why core with ports, not a media spore

Products, the storefront API, search documents (0014), the admin product page and later
collections and the UCP adapter all need media. If media were a spore, every one of them
would handle "no media installed", and each DAM spore would invent its own way of
attaching images to products. Shopify and Medusa both keep product media in core and
make only storage swappable.

What can be replaced, and where:

| Layer | Core or port |
| --- | --- |
| Assets, metadata, product and variant attachments, usage tracking | **Core** (`hypha-media`, the product attachments) |
| Where bytes live | **`BlobStore`**: S3 API (R2, S3, MinIO); a Workers R2-binding adapter later |
| URLs, sizes, the CDN | **`MediaUrls`**: Cloudflare, imgproxy, plain |
| Where assets come from | Uploads (built in), or an **external source** spore (Cloudinary, Bynder, Sanity…) |

The spore system isn't built yet (0002 §6), and designing it just to ship media would be
a bigger job than media. The ports are the extension points from day one; our own
storage can move into a spore later without changing the model.

## 2. The `Asset` aggregate (`hypha-media`, leaf)

| Field | Notes |
| --- | --- |
| `id` | `asset_…` |
| `kind` | `image`, `video`, `file`: decided from the stored file, not from what the client claimed |
| `status` | `pending` (created, not yet uploaded), `ready`, `failed` (the upload didn't match what was declared). Only `ready` assets can be attached |
| `source` | `uploaded { key, version }` or `external { provider, external_id, url }` (section 7) |
| `filename` | Sanitised, kept for the URL (readable, good for SEO, as Shopify does) and downloads |
| `mime_type`, `byte_size`, `checksum` | From the stored object. The SHA-256 checksum finds duplicates (the library offers the existing asset instead of a second copy) |
| `width`, `height` | Images and videos. Sizes for display come from URL transforms (section 4) |
| `duration_ms` | Videos, when known. *Later* |
| `alt`, `title` | Up to 512 characters each. `alt` describes the image for screen readers and search engines; `title` is the merchant's label in the library |
| `visibility` | `public` or `private` (M6) |
| `created_at`, `updated_at`, `version` | `Versioned` like every aggregate |

Rules: an asset becomes `ready` only through `complete_upload` (or a source spore);
`replace_file` starts a new pending version and keeps the old one serving until the new
one is ready; `delete_asset` refuses while usages exist (`asset_in_use`, with the usages
in `details`).

Library organisation (tags, folders, collections of assets) is *later* (section 10):
the first version lists, searches by filename and title, and filters by kind.

## 3. Uploads

```text
admin ──POST /admin/assets {filename, mime_type, byte_size}──▶ Hypha   (asset: pending)
      ◀─ {asset, upload: {url, method: PUT, headers}} ─┘
admin ──PUT file──────────────────────────────────────────────▶ R2 / S3 / MinIO
admin ──POST /admin/assets/{id}/complete──▶ Hypha ──HEAD + ranged GET──▶ storage
      ◀─ asset: ready (type, size, dimensions read from the file) ─┘
```

- **Presigned PUT** with `Content-Type` and `Content-Length` signed in, so storage
  refuses a different size or type than the one declared. Valid for 15 minutes.
- **Why direct:** Workers limit request bodies, and proxying large files through the API
  costs time and memory on every host. The S3 signature (SigV4) is HMAC-SHA256 only, so
  signing works in Workers too.
- **Completing checks the file, not the request:** `HEAD` confirms the size; a ranged
  `GET` of the first 64 KB lets `imagesize` (pure Rust, builds for wasm) read the real
  format and dimensions from the file's header. A file whose bytes aren't the declared
  kind makes the asset `failed` and the object is deleted.
- **Limits** (configurable): images up to 20 MB, videos up to 1 GB, other files up to
  100 MB; image types JPEG, PNG, WebP, AVIF, GIF. SVG is refused
  (`unsupported_media_type`) until we can sanitise it (M12).
- **Abandoned uploads:** pending assets older than a day are deleted by a background
  sweep (with their objects, if any).
- **Imports by URL** (`POST /admin/assets {source_url}`): the host downloads the file and
  stores it as an upload. For moving catalogs from other platforms. *Later*.

## 4. Delivery and the CDN

A CDN keeps copies of files on servers near shoppers: the first request fetches from
storage (the origin), later ones are served from the nearby copy. Hypha's job is to make
URLs that cache well; any CDN in front then works.

- **Immutable URLs (M4):** the key holds the asset ID and file version, and an object is
  written with `Cache-Control: public, max-age=31536000, immutable`. A URL's content
  never changes, so caches never need clearing; deleting an asset leaves cached copies to
  expire. An optional `purge` on `MediaUrls` lets adapters that can clear caches do so.
- **One setting:** `HYPHA_MEDIA_URL`, the public base, e.g. `https://cdn.mystore.com`.
  The API builds every public URL from it.
- **Sizes through the URL:** `MediaUrls::image(asset, Transform { width, height, fit,
  format })`. Responses carry the original URL plus a few standard sizes, and the SDK
  builds others. `format: auto` serves AVIF or WebP to browsers that take them.

| Host | Storage | CDN and sizes | Setup |
| --- | --- | --- | --- |
| Cloudflare | R2 | R2 custom domain (Cloudflare's CDN), Image Transformations (`/cdn-cgi/image/width=800,format=auto/<key>`) | Bind a domain to the bucket, enable transformations |
| VPS / AWS | S3 or MinIO | CloudFront, bunny.net or Cloudflare in front; imgproxy for sizes (signed URLs, so strangers can't make the server resize at will) | Point the CDN at the origin |
| Local | MinIO (compose) | None; originals only (`plain` adapter), or imgproxy in compose | `docker compose up -d` |

- **Private assets (M6):** stored under `private/…`, outside what the public domain
  serves; `BlobStore::signed_get(key, ttl)` gives a short-lived URL to someone allowed to
  see the file.
- **Why a separate media domain:** caching rules of its own, and files uploaded by staff
  (PDFs, and SVG once supported) never run on the admin's or storefront's origin.

## 5. Attaching media to products (M7)

- `Product` gains `media: Vec<ProductMedia>` (ordered; the first is featured) and each
  `Variant` gains `media: Option<AssetId>`, which must be one of the product's items.
  Removing an item from the product clears variants that pointed at it.
- At most 250 items per product (Shopify's limit), so a product stays one load.
- **Layers:** `hypha-products` stores `AssetId`s without knowing assets (leaf rule:
  plain values only). The commands that attach, reorder and detach (`set_product_media`,
  `set_variant_media`) load the assets, check they exist, are `ready` and public, and
  write the product and its usages in one batch. They need both crates, so they live one
  layer up, in a new composing crate **`hypha-merchandising`** (*Decided*): it holds the
  commands that present products (media now; collections and featured products later),
  as `hypha-cart` composes products and pricing. Products and media stay independent
  leaf crates; the alternative, an edge `hypha-products` → `hypha-media`, would tie
  products to media for good and leave later owners (collections, the store's logo)
  without a shared home.
- **API:** `ProductResponse` gains `media: [{asset, url, alt, width, height, sizes}]`
  and variants gain `media` (an asset ID); the list summary (0012 L7) gains
  `featured_media` with one small size. Storefront DTOs get the same, public assets only.

## 6. Usage tracking (M8)

```text
AssetUsage { asset: AssetId, owner: Owner { kind: "product", id: "prod_…" }, role: "featured" | "gallery" | "variant" }
```

- Written by the attach commands through `StageAssetUsages` in the same batch as the
  product, so they can't drift apart on a crash or a race; replaced wholesale for an
  owner on every change, like product children.
- `owner.kind` is a string, not an enum: spores register their own owners
  (`spore_lookbook`) without `hypha-media` knowing them.
- **"Used by":** `GET /admin/assets/{id}/usages` (a page, 0012). Deleting an owner (a
  product, later) removes its usages in the same batch.
- **Why recorded:** deriving "used by" means asking every domain that might hold an asset
  ID, including spores, which media can't depend on. One table answers it, and the
  delete guard is one lookup.
- **The contract** checks usages are replaced, not appended, and that the delete guard
  sees them.

## 7. External DAMs (M9)

- A source spore (Cloudinary, Bynder, Sanity…) creates assets with
  `source: external { provider, external_id, url }` and keeps metadata (alt, dimensions,
  checksum if any) in sync through the commands any app uses (`create_external_asset`,
  `update_asset`), as `Actor::Agent` or an API key with `media:write`.
- `MediaUrls` routes external assets to their provider, which builds sizes with its own
  transforms (most DAMs have them). Hypha never copies the bytes.
- Products, usages, the storefront and search treat them like uploads, so a merchant on
  Cloudinary keeps "used by", alt text and the same API.
- Until spores exist, the same commands over the admin API let an external app do it.

## 8. Storage and local development

- **`BlobStore` port** (`hypha-media`): `presign_put`, `head`, `get_range`, `delete`,
  `signed_get`, `public_key_url`. `async fn`, no `Send` bound (Workers), like the
  repositories. The S3 adapter lives in a new storage-adapter crate (`hypha-store-s3`),
  using the host's HTTP client through a small port so it builds for Workers too.
- **Contract:** `hypha_testkit::contract::blob_store` (presign then upload, `head`,
  ranges, delete, signed URLs expiring) runs against MinIO in CI and locally, so R2/S3
  behaviour differences show up as test failures. The testkit gets an in-memory
  `BlobStore` for the domain crates' fast tests.
- **`compose.yaml`** gains MinIO (S3 API on 9000, console on 9001) and a one-shot `mc`
  container that creates the `hypha-media` bucket with public read on `assets/` only.
  `.cargo/config.toml` and `apps/admin/.env.example` get the defaults:
  `HYPHA_S3_ENDPOINT=http://localhost:9000`, bucket, keys, and
  `HYPHA_MEDIA_URL=http://localhost:9000/hypha-media`.
- **Postgres:** `assets`, `asset_usages (asset_id, owner_kind, owner_id, role)` with an
  index on `asset_id`, `product_media (product_id, position, asset_id)` and
  `variants.media_asset_id`. No foreign keys from products into media (0002 §7), so
  either side can move to its own database later.

## 9. API and admin

| Route | Scope | Notes |
| --- | --- | --- |
| `POST /admin/assets` | `media:write` | Creates a pending asset, returns the upload URL |
| `POST /admin/assets/{id}/complete` | `media:write` | Checks the file, makes the asset ready |
| `GET /admin/assets?kind&q&cursor&limit` | `media:read` | The library, newest first (0012) |
| `GET /admin/assets/{id}` | `media:read` | With URLs and sizes |
| `PATCH /admin/assets/{id}` | `media:write` | Alt, title, visibility (Merge Patch) |
| `POST /admin/assets/{id}/replace` | `media:write` | A new file version, same asset |
| `DELETE /admin/assets/{id}` | `media:write` | `409 asset_in_use` with the usages |
| `GET /admin/assets/{id}/usages` | `media:read` | "Used by" |
| `PUT /admin/products/{id}/media` | `products:write` | The ordered list of asset IDs |
| `PUT /admin/products/{id}/variants/{variant}/media` | `products:write` | One asset ID or `null` |

New scopes `media:read` and `media:write` (the write implies the read, as for the others).

**Admin:** a Media page (grid, kind filter, search, upload by drag and drop, a detail
panel with alt, title, dimensions, "used by" and replace), and a Media section on the
product page (upload or pick from the library, drag to reorder, the first is featured,
choose each variant's image). Uploads go from the browser straight to storage with the
URL the admin's server got from Hypha, so the session token stays server-side (0011 P4).

## 10. Phases

1. **Assets and storage:** `hypha-media`, `BlobStore` and its contract, the S3 adapter,
   MinIO in compose, uploads, `plain` and Cloudflare `MediaUrls`, the asset routes, the
   admin Media page.
2. **Products:** attachments, usages, the delete guard, media in product responses and
   list summaries, the product page's Media section.
3. **Library and sources:** imgproxy adapter, imports by URL, external-asset commands for
   DAM spores, tags or folders, bulk actions.
4. **Video:** duration and poster frames; streaming via a provider (Cloudflare Stream,
   Mux) as a source.

## 11. What others do

- **Shopify:** product media (images, videos, 3D models) in an ordered list, a variant
  shows one of them, alt text on the media; a Files library; everything served from
  `cdn.shopify.com` with versioned URLs and size parameters. Storage is not replaceable.
- **Medusa:** a File module with providers (local, S3); product images are URLs with a
  rank. Swappable storage, but no library or usage tracking.
- **We take:** Shopify's model (ordered media, variant image, a library, CDN-friendly
  URLs) with Medusa-style swappable storage, plus usage tracking and external sources.

## 12. Hard to reverse

- **M2, M7:** the asset and product-media JSON shapes, once storefronts render them.
- **M4:** the key and URL scheme: URLs end up in caches, search engines and emails.
- **M8:** recording usages means every owner's commands write them; switching to derived
  usages later would mean a migration and a query per owner type.
- **M7 layering:** a composing `hypha-merchandising` (section 5). Moving the commands
  into products later would add the edge this avoids.
