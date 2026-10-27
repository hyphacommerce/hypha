---
title: "Your first spore: a wishlist"
description: Build a spore with a table of its own, storefront routes for signed-in shoppers, an operator setting and an admin block, tested against a real host.
sidebar:
  order: 4
---

This walks through [hyphacommerce/spore-wishlist](https://github.com/hyphacommerce/spore-wishlist),
a complete spore in about three hundred lines: signed-in shoppers save products, staff see
a customer's list in the admin. Read [Write a spore](/docs/guides/spores/) first for what a
spore is; this page is about doing it. Clone the repository and read along.

## 1. The crate

A spore is a Rust crate of its own, depending on one crate, `hypha-spore`, the facade
every spore builds on. Its features say what the spore uses: `postgres` for a table of its
own, `http` for routes.

```toml
[package]
name = "hypha-spore-wishlist"
edition = "2024"

[workspace]   # a crate of its own, not part of a host's workspace

[dependencies]
hypha-spore = { git = "https://github.com/hyphacommerce/hypha", branch = "main", features = ["postgres", "http"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
uuid = "1"

[dev-dependencies]
hypha-host-axum = { git = "https://github.com/hyphacommerce/hypha", branch = "main", features = ["testing"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

It must build for `wasm32-unknown-unknown`, because Hypha runs on Cloudflare Workers too:
no Tokio, threads, files or system time in the crate. Time, IDs and randomness come from
the spore's environment.

## 2. The spore

`src/lib.rs` names it and says what it brings. The name is for good: it's the key of the
spore's app (`spore_wishlist`), the prefix of its tables and the path of its routes.

```rust
pub struct Wishlist {
    pub env: SporeEnv,
    pub max_items: u32,
}

impl Spore for Wishlist {
    const NAME: &'static str = "wishlist";
    const SCOPES: &'static [Scope] = &[Scope::ProductsRead, Scope::CustomersRead];

    fn title() -> &'static str { "Wishlist" }

    fn migrations() -> MigrationSet {
        MigrationSet {
            module: "spore_wishlist",
            migrations: &[Migration {
                version: 1,
                name: "items",
                sql: "CREATE TABLE spore_wishlist_items (
                    customer_id uuid   NOT NULL REFERENCES customers (id) ON DELETE CASCADE,
                    product_id  uuid   NOT NULL REFERENCES products (id) ON DELETE CASCADE,
                    added_at    bigint NOT NULL,
                    PRIMARY KEY (customer_id, product_id)
                );",
            }],
        }
    }

    fn new(env: SporeEnv) -> Result<Self, SporeError> {
        let config: Config = env.config_as()?;
        let max_items = config.max_items.unwrap_or(Self::DEFAULT_MAX_ITEMS);
        if max_items == 0 {
            return Err(SporeError::Config { key: Some("max_items".into()), message: "must be at least 1".into() });
        }
        Ok(Wishlist { env, max_items })
    }

    type PaymentProviders = ();
    type Handlers = ();
    fn payment_providers(&self) {}
    fn handlers(&self) {}
}
```

Three rules show here:

- **Own tables only.** The migration may create objects named `spore_wishlist_*` and
  reference core's tables with `ON DELETE CASCADE` or `SET NULL`, and nothing else. The
  API checks the catalog after each migration and refuses to start otherwise. A row
  belongs to a customer and a product, and goes when either goes.
- **Append-only migrations.** A change is a new `Migration`, never an edit of one that
  shipped.
- **Operator settings** come from the spore's section of `hypha.config.toml`,
  `[spores.wishlist]`, read with `env.config_as()` into a struct with
  `deny_unknown_fields`, so a typo stops the API with the key named. `new` runs once at
  startup; a failure there stops the API, naming the spore.

`SCOPES` are what the spore acts with when it reads core: here products and customers.

## 3. The table (`src/repo.rs`)

A spore reads and writes its own table through a repository trait of its own, implemented
for Hypha's store. The trait is yours, so Rust's orphan rule lets you implement it for a
type from another crate.

```rust
pub trait WishlistRepo {
    async fn list(&self, customer: CustomerId) -> Result<Vec<Item>, RepoError>;
    async fn add(&self, customer: CustomerId, product: ProductId, now: Timestamp) -> Result<bool, RepoError>;
    async fn remove(&self, customer: CustomerId, product: ProductId) -> Result<bool, RepoError>;
}

pub trait HasWishlist {
    type Wishlist: WishlistRepo;
    fn wishlist(&self) -> &Self::Wishlist;
}

impl<C: PgConnection> WishlistRepo for PgStore<C> {
    async fn add(&self, customer: CustomerId, product: ProductId, now: Timestamp) -> Result<bool, RepoError> {
        let mut conn = self.conn().await;
        let written = conn.db()
            .execute_typed(
                "INSERT INTO spore_wishlist_items (customer_id, product_id, added_at)
                 VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
                &[(&customer.as_uuid(), Type::UUID), (&product.as_uuid(), Type::UUID), (&now.unix_micros(), Type::INT8)],
            )
            .await
            .map_err(|e| spore_repo_error(e, &[]))?;
        Ok(written == 1)
    }
    // list, remove: the same shape
}
```

`HasWishlist` is the accessor the handlers bound their ports on, the way every Hypha
command does with `HasProducts` or `HasOrders`: a handler asks for `S::Ports: HasWishlist`
and gets the request's connection. `spore_repo_error` turns the driver's errors into
Hypha's `RepoError`; a spore with unique constraints of its own names them there so a
violation becomes `Duplicate(key)`.

Core data is never touched this way. Products and customers are read through core's ports
(`ports.products().get_many(..)`), so core's schema stays core's.

## 4. The routes (`src/routes.rs`)

Routes are declared once, at the crate root, under the spore's name:

```rust
hypha_spore::http::spore_routes! { crate::wishlist_routes for Wishlist:
    [storefront, GET, "/storefront/spores/wishlist/items", routes::list],
    [storefront, PUT, "/storefront/spores/wishlist/items/{product}", routes::add],
    [storefront, DELETE, "/storefront/spores/wishlist/items/{product}", routes::remove],
}
```

A handler looks like one of Hypha's own. It takes the host's state and a caller, and its
first line authenticates the caller for the request's ports. These routes are for a
signed-in shopper, so the caller is `CustomerCaller`: the publishable key and the
customer's session, both; without the session the answer is `401 account_required`.

```rust
#[utoipa::path(
    put,
    path = "/storefront/spores/wishlist/items/{product}",
    params(("product" = ProductId, Path, description = "The product to save")),
    responses((status = 200, description = "OK", body = WishlistResponse)),
    extensions(("x-hypha-scopes" = json!([])), ("x-hypha-customer-session" = json!(true))),
)]
pub async fn add<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: CustomerCaller,
    PathId(product): PathId<ProductId>,
) -> Result<Json<WishlistResponse>, ApiError>
where
    for<'a> S::Ports<'a>: HasWishlist,
{
    let (ports, me) = caller.authenticate(&state).await?;
    let spore = hypha_spore::http::spore::<Wishlist, S>(&state)?;
    let customer = me.customer.id();
    let found = ports.products().get(product).await?;
    if !found.is_some_and(|p| p.status() == ProductStatus::Active) {
        return Err(ApiError::of(StatusCode::NOT_FOUND, &WishlistError::ProductNotFound(product)));
    }
    let saved = ports.wishlist().list(customer).await?;
    if !saved.iter().any(|i| i.product == product) && saved.len() >= spore.max_items as usize {
        return Err(ApiError::of(StatusCode::CONFLICT, &WishlistError::Full { max: spore.max_items }));
    }
    ports.wishlist().add(customer, product, spore.env.now()).await?;
    respond(spore, &ports, customer).await
}
```

- The `#[utoipa::path]` annotation is how the host documents the route: it lands in
  `openapi/storefront.json` under the spore's tag, so the TypeScript SDK can be generated
  with it. `x-hypha-customer-session` marks it as needing the shopper signed in.
- `hypha_spore::http::spore::<Wishlist, S>(&state)` is the spore built at startup: its
  settings and its environment. Time comes from `spore.env.now()` (the environment is a
  `Clock`), never from the system.
- Errors of the spore's own are an enum with `thiserror` and `ErrorCode` (a stable
  snake_case code per variant), answered with `ApiError::of(status, &error)`, so clients
  match on `wishlist_full` the way they match on Hypha's codes. `RepoError` converts with
  `?`.
- The answer is the whole list, with each product's handle and title read through core's
  ports, archived products left out. A storefront never needs a second read.

While staff have the spore disabled in the admin, every one of its routes answers
`403 app_disabled` without running.

## 5. The admin block (`src/ui.rs`)

A spore adds to the admin by answering blocks: declarative trees the admin renders with its
own components, on the same targets apps use. This one lists the customer's products on
their page.

```rust
impl<P: HasProducts + HasWishlist> SporeUi<P> for Wishlist {
    fn blocks() -> Vec<UiBlock> {
        vec![UiBlock { id: "items", target: Target::CustomerDetails }]
    }

    async fn block(&self, id: &str, call: UiCall<'_>, ports: &P) -> Result<Value, UiError> {
        let customer = call.context.customer.ok_or(UiError::NotFound)?;
        let saved = ports.wishlist().list(customer).await.map_err(|e| UiError::Failed(e.to_string()))?;
        // … load the products, then:
        Ok(json!({ "version": 1, "root": { "type": "card", "title": format!("Wishlist ({})", items.len()),
                                           "children": [{ "type": "list", "items": items }] } }))
    }
}
```

`call.ctx` is the spore acting as its app, with its scopes as far as the staff member
holds them: a staff member who can't read products doesn't see them here either.

## 6. Testing against a real host (`tests/routes.rs`)

The test kit builds a Hypha host with your spore compiled in, on a database of its own per
test (Hypha's compose `postgres-test`), and lets you drive it over HTTP.

```rust
hypha_host_axum::deployment! { enum WithWishlist {
    spores: [Wishlist],
    routes: [wishlist_routes],
} }

#[tokio::test]
async fn a_shopper_saves_and_removes_products_within_the_limit() {
    let app = app_with::<WithWishlist>("[spores.wishlist]\nmax_items = 2").await;
    let pk = publishable(&app).await;
    let mug = product(&app, "mug", true).await;
    let (session, _) = shopper(&app, &pk, "nina@example.com").await;
    let (status, json) = as_shopper(&app, &pk, &session, "PUT", &format!("/storefront/spores/wishlist/items/{mug}")).await;
    assert_eq!(status, 200, "{json}");
    assert_eq!(handles(&json), ["mug"]);
}
```

`app_with` takes the spore's `hypha.config.toml` text, so the limit is tested at 2. The
shopper signs in the way a real one does: a code is emailed, and the test kit records
what the host sent (`app.emails()`), so the test reads the code from there. The block is
checked through the same routes the admin uses: `/admin/app-extensions?target=customer.details`
lists it, and `/admin/app-extensions/blocks/spore_wishlist/items?customer_id=…` renders it.

A configuration the spore refuses (`max_items = 0`) fails the host's build, and the test
asserts exactly that.

## 7. Building it into a store

```
hypha spores add wishlist --crate hypha-spore-wishlist \
  --type hypha_spore_wishlist::Wishlist --routes hypha_spore_wishlist::wishlist_routes \
  --git https://github.com/hyphacommerce/spore-wishlist
hypha deploy
```

The deploy clones the spore at its commit, writes the deployment's own host crate with it,
and builds the API from that. On the first start the spore's migration runs, its app
`spore_wishlist` is registered, and the routes answer. In the admin, Apps → Wishlist has
the switch that disables it.

## Where to go from here

- **React to core**: an `EventHandler` in `Handlers` hears events such as `order.placed`,
  at least once. A wishlist could drop a product once it's bought.
- **Merchant settings**: `settings_schema()` draws a form in the admin; `env.settings()`
  reads what the merchant saved. A limit the merchant sets instead of the operator.
- **Custom fields**: `custom_fields()` declares fields in the spore's namespace on core's
  objects, synced at startup, for data other clients should see.
- **A storefront element**: a web component the storefront mounts, served from a public
  route of the spore. The [Stripe spore](https://github.com/hyphacommerce/spore-stripe)
  has one, and a payment provider, and a webhook: it's the next thing to read.
