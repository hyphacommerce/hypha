use std::collections::{HashMap, HashSet};

use futures_util::future::try_join;
use hypha_core::{
    BasisPoints, CatalogId, CategoryId, CountryCode, Direction, Keyset, MarketId, Page,
    PageRequest, Price, PriceListId, ProductId, Recorded, RepoError, SortKey, VariantId, Version,
    Versioned, stamp_insert, stamp_update,
};
use hypha_markets::{
    Adjustment, AppliesTo, Catalog, CatalogContext, CatalogRepo, CatalogSnapshot, CompareAtMode,
    FixedPrice, FixedPriceFilter, Market, MarketRepo, MarketSnapshot, PriceList, PriceListFilter,
    PriceListKind, PriceListPhase, PriceListRepo, PriceListSnapshot, PriceListSort,
    PriceListStatus,
};
use tokio_postgres::{
    Row,
    types::{ToSql, Type},
};
use uuid::Uuid;

use crate::{
    columns::{
        col, currency_from_db, micros, opt_micros, opt_ts, stamp_columns, stamps_from_row, ts,
        version_from_db, version_to_db, write_stamps,
    },
    error::{corrupt, repo_error},
    outbox::insert_events,
    store::Queries,
    store::{PgConnection, PgStore, consistent_read, in_transaction},
};

// ---------- markets ----------

impl<C: PgConnection> MarketRepo for PgStore<C> {
    async fn get(&self, id: MarketId) -> Result<Option<Market>, RepoError> {
        let mut conn = self.conn().await;
        consistent_read!(conn, |tx| {
            Ok(load_markets(tx, Markets::Id(id.as_uuid())).await?.pop())
        })
    }

    async fn list_all(&self) -> Result<Vec<Market>, RepoError> {
        let mut conn = self.conn().await;
        consistent_read!(conn, |tx| load_markets(tx, Markets::All).await)
    }

    async fn get_by_country(&self, country: CountryCode) -> Result<Option<Market>, RepoError> {
        let mut conn = self.conn().await;
        consistent_read!(conn, |tx| {
            Ok(load_markets(tx, Markets::Country(country.as_str()))
                .await?
                .pop())
        })
    }

    async fn insert(&self, market: &mut Market) -> Result<(), RepoError> {
        stamp_insert(market, self.now());
        let s = market.snapshot();
        let events = market.take_events();
        let stamp = self.event_stamp();
        let version = Version::NEW.next();
        in_transaction!(self, |tx| {
            insert_market(tx, &s).await?;
            insert_events(tx, &events, Some(version), stamp).await
        })?;
        market.set_version(version);
        Ok(())
    }

    async fn update(&self, market: &mut Market) -> Result<(), RepoError> {
        stamp_update(market, self.now());
        let s = market.snapshot();
        let events = market.take_events();
        let stamp = self.event_stamp();
        let version = in_transaction!(self, |tx| {
            let version = update_market(tx, &s).await?;
            insert_events(tx, &events, Some(version), stamp).await?;
            Ok(version)
        })?;
        market.set_version(version);
        Ok(())
    }
}

/// `MarketRepo::insert` inside the caller's transaction: the row, then its countries.
pub(crate) async fn insert_market(tx: &impl Queries, s: &MarketSnapshot) -> Result<(), RepoError> {
    tx.query_typed(
        "INSERT INTO markets (id, version, name, currency, prices_include_tax)
         VALUES ($1, 1, $2, $3, $4)",
        &[
            (&s.id.as_uuid(), Type::UUID),
            (&s.name, Type::TEXT),
            (&s.currency.as_str(), Type::TEXT),
            (&s.prices_include_tax, Type::BOOL),
        ],
    )
    .await
    .map_err(repo_error)?;
    write_countries(tx, s).await?;
    write_stamps(tx, "markets", "id", &s.id.as_uuid(), Type::UUID, s.stamps).await?;
    Ok(())
}

/// `MarketRepo::update` inside the caller's transaction; the new version. The version
/// check comes first; the countries are only replaced if it passes.
pub(crate) async fn update_market(
    tx: &impl Queries,
    s: &MarketSnapshot,
) -> Result<Version, RepoError> {
    let bumped = tx
        .query_typed_opt(
            "UPDATE markets SET version = version + 1, name = $3, currency = $4,
                 prices_include_tax = $5
             WHERE id = $1 AND version = $2 RETURNING version",
            &[
                (&s.id.as_uuid(), Type::UUID),
                (&version_to_db(s.version)?, Type::INT8),
                (&s.name, Type::TEXT),
                (&s.currency.as_str(), Type::TEXT),
                (&s.prices_include_tax, Type::BOOL),
            ],
        )
        .await
        .map_err(repo_error)?;
    let row = bumped.ok_or(RepoError::StaleVersion)?;
    tx.query_typed(
        "DELETE FROM market_countries WHERE market_id = $1",
        &[(&s.id.as_uuid(), Type::UUID)],
    )
    .await
    .map_err(repo_error)?;
    write_countries(tx, s).await?;
    let written = version_from_db(col(&row, "version")?)?;
    write_stamps(tx, "markets", "id", &s.id.as_uuid(), Type::UUID, s.stamps).await?;
    Ok(written)
}

async fn write_countries(tx: &impl Queries, s: &MarketSnapshot) -> Result<(), RepoError> {
    let countries: Vec<&str> = s.countries.iter().map(|c| c.as_str()).collect();
    // One statement for all countries; `WITH ORDINALITY` keeps their order.
    tx.query_typed(
        "INSERT INTO market_countries (country, market_id, position)
         SELECT country, $1, (n - 1)::integer FROM unnest($2::text[]) WITH ORDINALITY AS t (country, n)",
        &[(&s.id.as_uuid(), Type::UUID), (&countries, Type::TEXT_ARRAY)],
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}

/// One market (`Some(id)`) or all of them, in 2 queries.
/// Which markets a load is for: the filter on `markets m` both of its queries share,
/// so they go out together (pipelined) and read the same snapshot.
enum Markets<'a> {
    All,
    Id(Uuid),
    Country(&'a str),
}

async fn load_markets(client: &impl Queries, which: Markets<'_>) -> Result<Vec<Market>, RepoError> {
    let (filter, param): (&str, [(&(dyn ToSql + Sync), Type); 1]) = match &which {
        Markets::All => ("$1::uuid IS NULL", [(&None::<Uuid>, Type::UUID)]),
        Markets::Id(id) => ("m.id = $1", [(id, Type::UUID)]),
        Markets::Country(country) => (
            "m.id = (SELECT market_id FROM market_countries WHERE country = $1)",
            [(country, Type::TEXT)],
        ),
    };
    let markets_sql = format!(
        "SELECT m.id, m.version, m.name, m.currency, m.prices_include_tax, {} FROM markets m
         WHERE {filter} ORDER BY m.id",
        stamp_columns("m."),
    );
    let countries_sql = format!(
        "SELECT c.market_id, c.country FROM market_countries c
         JOIN markets m ON m.id = c.market_id
         WHERE {filter} ORDER BY c.market_id, c.position"
    );
    let (rows, country_rows) = try_join(
        client.query_typed(&markets_sql, &param),
        client.query_typed(&countries_sql, &param),
    )
    .await
    .map_err(repo_error)?;
    let mut countries: HashMap<Uuid, Vec<CountryCode>> = HashMap::new();
    for row in country_rows {
        let code: String = col(&row, "country")?;
        let country = CountryCode::new(&code).map_err(corrupt)?;
        countries
            .entry(col(&row, "market_id")?)
            .or_default()
            .push(country);
    }
    rows.iter()
        .map(|row| {
            let id: Uuid = col(row, "id")?;
            let snapshot = MarketSnapshot {
                id: MarketId::from_uuid(id),
                version: version_from_db(col(row, "version")?)?,
                stamps: stamps_from_row(row)?,
                name: col(row, "name")?,
                countries: countries.remove(&id).unwrap_or_default(),
                currency: currency_from_db(col(row, "currency")?)?,
                prices_include_tax: col(row, "prices_include_tax")?,
            };
            Market::restore(snapshot).map_err(|e| corrupt(format!("market {id}: {e}")))
        })
        .collect()
}

// ---------- catalogs ----------

fn context_to_db(context: CatalogContext) -> (&'static str, Uuid) {
    match context {
        CatalogContext::Market(id) => ("market", id.as_uuid()),
    }
}

fn catalog_from_row(row: &Row, price_lists: Vec<PriceListId>) -> Result<Catalog, RepoError> {
    let kind: String = col(row, "context_kind")?;
    let context = match kind.as_str() {
        "market" => CatalogContext::Market(MarketId::from_uuid(col(row, "context_id")?)),
        other => return Err(corrupt(format!("catalog context kind {other:?}"))),
    };
    let id: Uuid = col(row, "id")?;
    Catalog::restore(CatalogSnapshot {
        id: CatalogId::from_uuid(id),
        version: version_from_db(col(row, "version")?)?,
        stamps: stamps_from_row(row)?,
        context,
        price_lists,
    })
    .map_err(|e| corrupt(format!("catalog {id}: {e}")))
}

/// Which catalogs a load is for: the filter on `catalogs c` both its queries share, so
/// they go out together (pipelined).
enum Catalogs {
    All,
    Id(Uuid),
    Context(&'static str, Uuid),
}

/// The catalogs `which` names with their price lists, oldest first, in 2 pipelined queries.
async fn load_catalogs(client: &impl Queries, which: Catalogs) -> Result<Vec<Catalog>, RepoError> {
    let none = None::<Uuid>;
    let (filter, params): (&str, Vec<(&(dyn ToSql + Sync), Type)>) = match &which {
        Catalogs::All => ("$1::uuid IS NULL", vec![(&none, Type::UUID)]),
        Catalogs::Id(id) => ("c.id = $1", vec![(id, Type::UUID)]),
        Catalogs::Context(kind, id) => (
            "c.context_kind = $1 AND c.context_id = $2",
            vec![(kind, Type::TEXT), (id, Type::UUID)],
        ),
    };
    let catalogs_sql = format!(
        "SELECT c.id, c.version, c.context_kind, c.context_id, {} FROM catalogs c
         WHERE {filter} ORDER BY c.id",
        stamp_columns("c."),
    );
    let lists_sql = format!(
        "SELECT l.catalog_id, l.price_list_id FROM catalog_price_lists l
         JOIN catalogs c ON c.id = l.catalog_id
         WHERE {filter} ORDER BY l.catalog_id, l.price_list_id"
    );
    let (rows, list_rows) = try_join(
        client.query_typed(&catalogs_sql, &params),
        client.query_typed(&lists_sql, &params),
    )
    .await
    .map_err(repo_error)?;
    let mut lists: HashMap<Uuid, Vec<PriceListId>> = HashMap::new();
    for row in list_rows {
        lists
            .entry(col(&row, "catalog_id")?)
            .or_default()
            .push(PriceListId::from_uuid(col(&row, "price_list_id")?));
    }
    rows.iter()
        .map(|row| {
            let id: Uuid = col(row, "id")?;
            catalog_from_row(row, lists.remove(&id).unwrap_or_default())
        })
        .collect()
}

impl<C: PgConnection> CatalogRepo for PgStore<C> {
    async fn get(&self, id: CatalogId) -> Result<Option<Catalog>, RepoError> {
        let mut conn = self.conn().await;
        consistent_read!(conn, |tx| {
            Ok(load_catalogs(tx, Catalogs::Id(id.as_uuid())).await?.pop())
        })
    }

    async fn get_by_context(&self, context: CatalogContext) -> Result<Option<Catalog>, RepoError> {
        let (kind, context_id) = context_to_db(context);
        let mut conn = self.conn().await;
        consistent_read!(conn, |tx| {
            Ok(load_catalogs(tx, Catalogs::Context(kind, context_id))
                .await?
                .pop())
        })
    }

    async fn list_all(&self) -> Result<Vec<Catalog>, RepoError> {
        let mut conn = self.conn().await;
        consistent_read!(conn, |tx| load_catalogs(tx, Catalogs::All).await)
    }

    async fn product_page(
        &self,
        catalog: CatalogId,
        page: PageRequest<ProductId>,
    ) -> Result<Page<ProductId>, RepoError> {
        let after = page.after.map(|id| id.as_uuid());
        let fetch = i64::from(page.limit.get()) + 1;
        let mut conn = self.conn().await;
        let rows = conn
            .db()
            .query_typed(
                "SELECT product_id FROM catalog_products
                 WHERE catalog_id = $1 AND ($2::uuid IS NULL OR product_id < $2)
                 ORDER BY product_id DESC LIMIT $3",
                &[
                    (&catalog.as_uuid(), Type::UUID),
                    (&after, Type::UUID),
                    (&fetch, Type::INT8),
                ],
            )
            .await
            .map_err(repo_error)?;
        let ids = rows
            .iter()
            .map(|r| Ok(ProductId::from_uuid(col(r, "product_id")?)))
            .collect::<Result<Vec<_>, RepoError>>()?;
        Ok(Page::from_overfetch(ids, page.limit))
    }

    async fn insert(&self, catalog: &mut Catalog) -> Result<(), RepoError> {
        stamp_insert(catalog, self.now());
        let s = catalog.snapshot();
        let events = catalog.take_events();
        let stamp = self.event_stamp();
        let version = Version::NEW.next();
        in_transaction!(self, |tx| {
            insert_catalog(tx, &s).await?;
            insert_events(tx, &events, Some(version), stamp).await
        })?;
        catalog.set_version(version);
        Ok(())
    }

    async fn update(&self, catalog: &mut Catalog) -> Result<(), RepoError> {
        stamp_update(catalog, self.now());
        let s = catalog.snapshot();
        let events = catalog.take_events();
        let stamp = self.event_stamp();
        let version = in_transaction!(self, |tx| {
            let version = update_catalog(tx, &s).await?;
            insert_events(tx, &events, Some(version), stamp).await?;
            Ok(version)
        })?;
        catalog.set_version(version);
        Ok(())
    }

    async fn add_products(
        &self,
        catalog: CatalogId,
        products: &[ProductId],
    ) -> Result<(), RepoError> {
        in_transaction!(self, |tx| add_catalog_products(tx, catalog, products).await)
    }

    async fn remove_products(
        &self,
        catalog: CatalogId,
        products: &[ProductId],
    ) -> Result<(), RepoError> {
        in_transaction!(self, |tx| remove_catalog_products(tx, catalog, products)
            .await)
    }

    async fn included_products(
        &self,
        catalog: CatalogId,
        products: &[ProductId],
    ) -> Result<HashSet<ProductId>, RepoError> {
        if products.is_empty() {
            return Ok(HashSet::new());
        }
        let ids: Vec<Uuid> = products.iter().map(|p| p.as_uuid()).collect();
        let mut conn = self.conn().await;
        let rows = conn
            .db()
            .query_typed(
                "SELECT product_id FROM catalog_products WHERE catalog_id = $1 AND product_id = ANY($2)",
                &[(&catalog.as_uuid(), Type::UUID), (&ids, Type::UUID_ARRAY)],
            )
            .await
            .map_err(repo_error)?;
        rows.iter()
            .map(|r| Ok(ProductId::from_uuid(col(r, "product_id")?)))
            .collect()
    }
}

/// `CatalogRepo::insert` inside the caller's transaction: the row, then its price lists.
pub(crate) async fn insert_catalog(
    tx: &impl Queries,
    s: &CatalogSnapshot,
) -> Result<(), RepoError> {
    let (kind, context_id) = context_to_db(s.context);
    tx.query_typed(
        "INSERT INTO catalogs (id, version, context_kind, context_id) VALUES ($1, 1, $2, $3)",
        &[
            (&s.id.as_uuid(), Type::UUID),
            (&kind, Type::TEXT),
            (&context_id, Type::UUID),
        ],
    )
    .await
    .map_err(repo_error)?;
    write_catalog_lists(tx, s).await?;
    write_stamps(tx, "catalogs", "id", &s.id.as_uuid(), Type::UUID, s.stamps).await?;
    Ok(())
}

/// `CatalogRepo::update` inside the caller's transaction; the new version. The version
/// check comes first; the price lists are only replaced if it passes. Membership rows are
/// untouched.
pub(crate) async fn update_catalog(
    tx: &impl Queries,
    s: &CatalogSnapshot,
) -> Result<Version, RepoError> {
    let bumped = tx
        .query_typed_opt(
            "UPDATE catalogs SET version = version + 1
             WHERE id = $1 AND version = $2 RETURNING version",
            &[
                (&s.id.as_uuid(), Type::UUID),
                (&version_to_db(s.version)?, Type::INT8),
            ],
        )
        .await
        .map_err(repo_error)?;
    let row = bumped.ok_or(RepoError::StaleVersion)?;
    tx.query_typed(
        "DELETE FROM catalog_price_lists WHERE catalog_id = $1",
        &[(&s.id.as_uuid(), Type::UUID)],
    )
    .await
    .map_err(repo_error)?;
    write_catalog_lists(tx, s).await?;
    let written = version_from_db(col(&row, "version")?)?;
    write_stamps(tx, "catalogs", "id", &s.id.as_uuid(), Type::UUID, s.stamps).await?;
    Ok(written)
}

async fn write_catalog_lists(tx: &impl Queries, s: &CatalogSnapshot) -> Result<(), RepoError> {
    let lists: Vec<Uuid> = s.price_lists.iter().map(|l| l.as_uuid()).collect();
    tx.query_typed(
        "INSERT INTO catalog_price_lists (catalog_id, price_list_id)
         SELECT $1, l FROM unnest($2::uuid[]) AS t (l)",
        &[(&s.id.as_uuid(), Type::UUID), (&lists, Type::UUID_ARRAY)],
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}

/// `CatalogRepo::add_products` inside the caller's transaction.
pub(crate) async fn add_catalog_products(
    tx: &impl Queries,
    catalog: CatalogId,
    products: &[ProductId],
) -> Result<(), RepoError> {
    let ids: Vec<Uuid> = products.iter().map(|p| p.as_uuid()).collect();
    tx.query_typed(
        "INSERT INTO catalog_products (catalog_id, product_id)
         SELECT $1, product_id FROM unnest($2::uuid[]) AS t (product_id)
         ON CONFLICT DO NOTHING",
        &[(&catalog.as_uuid(), Type::UUID), (&ids, Type::UUID_ARRAY)],
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}

/// `CatalogRepo::remove_products` inside the caller's transaction.
pub(crate) async fn remove_catalog_products(
    tx: &impl Queries,
    catalog: CatalogId,
    products: &[ProductId],
) -> Result<(), RepoError> {
    let ids: Vec<Uuid> = products.iter().map(|p| p.as_uuid()).collect();
    tx.query_typed(
        "DELETE FROM catalog_products WHERE catalog_id = $1 AND product_id = ANY($2)",
        &[(&catalog.as_uuid(), Type::UUID), (&ids, Type::UUID_ARRAY)],
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}

// ---------- price lists ----------

/// `price_lists`' columns, as `price_list_from_row` reads them.
fn price_list_columns(prefix: &str) -> String {
    format!(
        "{prefix}id, {prefix}version, {prefix}name, {prefix}description, {prefix}currency,
         {prefix}kind, {prefix}status, {}, {}, {prefix}compare_at, {prefix}adjustment,
         {prefix}applies_to, {prefix}applies_to_ids, {}",
        micros(&format!("{prefix}starts_at")),
        micros(&format!("{prefix}ends_at")),
        stamp_columns(prefix),
    )
}

impl<C: PgConnection> PriceListRepo for PgStore<C> {
    async fn get(&self, id: PriceListId) -> Result<Option<PriceList>, RepoError> {
        let mut conn = self.conn().await;
        let row = conn
            .db()
            .query_typed_opt(
                &format!(
                    "SELECT {} FROM price_lists WHERE id = $1",
                    price_list_columns("")
                ),
                &[(&id.as_uuid(), Type::UUID)],
            )
            .await
            .map_err(repo_error)?;
        row.as_ref().map(price_list_from_row).transpose()
    }

    async fn list_all(&self) -> Result<Vec<PriceList>, RepoError> {
        let mut conn = self.conn().await;
        let rows = conn
            .db()
            .query_typed(
                &format!(
                    "SELECT {} FROM price_lists ORDER BY id",
                    price_list_columns("")
                ),
                &[],
            )
            .await
            .map_err(repo_error)?;
        rows.iter().map(price_list_from_row).collect()
    }

    async fn get_many(&self, ids: &[PriceListId]) -> Result<Vec<PriceList>, RepoError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let ids: Vec<Uuid> = ids.iter().map(|id| id.as_uuid()).collect();
        let mut conn = self.conn().await;
        let rows = conn
            .db()
            .query_typed(
                &format!(
                    "SELECT {} FROM price_lists WHERE id = ANY($1) ORDER BY id",
                    price_list_columns("")
                ),
                &[(&ids, Type::UUID_ARRAY)],
            )
            .await
            .map_err(repo_error)?;
        rows.iter().map(price_list_from_row).collect()
    }

    async fn list(
        &self,
        filter: &PriceListFilter,
        sort: PriceListSort,
        page: PageRequest<Keyset<PriceListId>>,
    ) -> Result<Page<PriceList>, RepoError> {
        let (cmp, dir) = match sort.direction() {
            Direction::Ascending => (">", "ASC"),
            Direction::Descending => ("<", "DESC"),
        };
        let mut name_key = None::<String>;
        let after = match &page.after {
            None => None,
            Some(after) => {
                match (&after.key, sort) {
                    (SortKey::Id, PriceListSort::Newest | PriceListSort::Oldest) => {}
                    (SortKey::Text(t), PriceListSort::NameAsc | PriceListSort::NameDesc) => {
                        name_key = Some(t.clone())
                    }
                    (key, sort) => {
                        return Err(RepoError::Backend(format!(
                            "a {key:?} position for a {sort:?} list"
                        )));
                    }
                }
                Some(after.id.as_uuid())
            }
        };
        let (position, order) = match sort {
            PriceListSort::Newest | PriceListSort::Oldest => {
                (format!("id {cmp} $1"), format!("id {dir}"))
            }
            PriceListSort::NameAsc | PriceListSort::NameDesc => (
                format!(r#"((lower(name) COLLATE "C"), id) {cmp} (($2::text COLLATE "C"), $1)"#),
                format!(r#"(lower(name) COLLATE "C") {dir}, id {dir}"#),
            ),
        };
        // Where a list stands at `$6`, as `PriceList::phase` judges it.
        let phase = match filter.phase {
            None => "TRUE",
            Some(PriceListPhase::Draft) => "status = 'draft'",
            Some(PriceListPhase::Scheduled) => "status = 'active' AND starts_at > $6",
            Some(PriceListPhase::Active) => {
                "status = 'active' AND (starts_at IS NULL OR starts_at <= $6)
                 AND (ends_at IS NULL OR ends_at > $6)"
            }
            Some(PriceListPhase::Expired) => {
                "status = 'active' AND (starts_at IS NULL OR starts_at <= $6) AND ends_at <= $6"
            }
        };
        let ids: Option<Vec<Uuid>> = filter
            .ids
            .as_ref()
            .map(|ids| ids.iter().map(|id| id.as_uuid()).collect());
        let kind = filter.kind.map(|k| k.as_str());
        let fetch = i64::from(page.limit.get()) + 1;
        let mut conn = self.conn().await;
        let rows = conn
            .db()
            .query_typed(
                &format!(
                    "SELECT {} FROM price_lists
                     WHERE ($1::uuid IS NULL OR {position})
                       AND ($4::text IS NULL OR kind = $4)
                       AND ($5::uuid[] IS NULL OR id = ANY($5))
                       AND {phase}
                     ORDER BY {order} LIMIT $3",
                    price_list_columns(""),
                    phase = phase.replace("$6", &ts(6)),
                ),
                &[
                    (&after, Type::UUID),
                    (&name_key, Type::TEXT),
                    (&fetch, Type::INT8),
                    (&kind, Type::TEXT),
                    (&ids, Type::UUID_ARRAY),
                    (&filter.at.unix_micros(), Type::INT8),
                ],
            )
            .await
            .map_err(repo_error)?;
        let lists = rows
            .iter()
            .map(price_list_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Page::from_overfetch(lists, page.limit))
    }

    async fn delete(&self, price_list: &mut PriceList) -> Result<(), RepoError> {
        let id = price_list.id().as_uuid();
        let events = price_list.take_events();
        let stamp = self.event_stamp();
        in_transaction!(self, |tx| {
            // The catalog reference refuses it while a catalog holds it; fixed prices go
            // with it.
            tx.query_typed(
                "DELETE FROM price_lists WHERE id = $1",
                &[(&id, Type::UUID)],
            )
            .await
            .map_err(repo_error)?;
            insert_events(tx, &events, None, stamp).await
        })
    }

    async fn fixed_price_counts(
        &self,
        lists: &[PriceListId],
    ) -> Result<HashMap<PriceListId, u64>, RepoError> {
        if lists.is_empty() {
            return Ok(HashMap::new());
        }
        let ids: Vec<Uuid> = lists.iter().map(|l| l.as_uuid()).collect();
        let mut conn = self.conn().await;
        let rows = conn
            .db()
            .query_typed(
                "SELECT price_list_id, count(*) AS n FROM price_list_prices
                 WHERE price_list_id = ANY($1) GROUP BY price_list_id",
                &[(&ids, Type::UUID_ARRAY)],
            )
            .await
            .map_err(repo_error)?;
        rows.iter()
            .map(|row| {
                let n: i64 = col(row, "n")?;
                Ok((
                    PriceListId::from_uuid(col(row, "price_list_id")?),
                    u64::try_from(n).map_err(corrupt)?,
                ))
            })
            .collect()
    }

    async fn fixed_prices_in(
        &self,
        lists: &[PriceListId],
        variants: &[VariantId],
    ) -> Result<HashMap<PriceListId, HashMap<VariantId, FixedPrice>>, RepoError> {
        if lists.is_empty() || variants.is_empty() {
            return Ok(HashMap::new());
        }
        let lists: Vec<Uuid> = lists.iter().map(|l| l.as_uuid()).collect();
        let ids: Vec<Uuid> = variants.iter().map(|v| v.as_uuid()).collect();
        let mut conn = self.conn().await;
        let rows = conn
            .db()
            .query_typed(
                "SELECT price_list_id, variant_id, amount, currency, compare_at
                 FROM price_list_prices WHERE price_list_id = ANY($1) AND variant_id = ANY($2)",
                &[(&lists, Type::UUID_ARRAY), (&ids, Type::UUID_ARRAY)],
            )
            .await
            .map_err(repo_error)?;
        let mut found: HashMap<PriceListId, HashMap<VariantId, FixedPrice>> = HashMap::new();
        for row in &rows {
            let (variant, fixed) = fixed_price_from_row(row)?;
            found
                .entry(PriceListId::from_uuid(col(row, "price_list_id")?))
                .or_default()
                .insert(variant, fixed);
        }
        Ok(found)
    }

    async fn product_categories(
        &self,
        products: &[ProductId],
    ) -> Result<HashMap<ProductId, HashSet<CategoryId>>, RepoError> {
        if products.is_empty() {
            return Ok(HashMap::new());
        }
        let ids: Vec<Uuid> = products.iter().map(|p| p.as_uuid()).collect();
        let mut conn = self.conn().await;
        // Up the parent pointers from each product's categories; UNION (not ALL) stops
        // at a category already reached.
        let rows = conn
            .db()
            .query_typed(
                "WITH RECURSIVE up (product_id, category_id) AS (
                     SELECT product_id, category_id FROM category_products
                     WHERE product_id = ANY($1)
                     UNION
                     SELECT up.product_id, c.parent_id FROM up
                     JOIN categories c ON c.id = up.category_id
                     WHERE c.parent_id IS NOT NULL
                 )
                 SELECT product_id, category_id FROM up",
                &[(&ids, Type::UUID_ARRAY)],
            )
            .await
            .map_err(repo_error)?;
        let mut found: HashMap<ProductId, HashSet<CategoryId>> = HashMap::new();
        for row in &rows {
            found
                .entry(ProductId::from_uuid(col(row, "product_id")?))
                .or_default()
                .insert(CategoryId::from_uuid(col(row, "category_id")?));
        }
        Ok(found)
    }

    async fn fixed_price_page(
        &self,
        list: PriceListId,
        filter: &FixedPriceFilter,
        page: PageRequest<VariantId>,
    ) -> Result<Page<(VariantId, FixedPrice)>, RepoError> {
        let after = page.after.map(|id| id.as_uuid());
        let fetch = i64::from(page.limit.get()) + 1;
        let product = filter.product().map(|p| p.as_uuid());
        let category = filter.category().map(|c| c.as_uuid());
        let q = filter.q();
        let mut conn = self.conn().await;
        let rows = conn
            .db()
            .query_typed(
                "SELECT f.variant_id, f.amount, f.currency, f.compare_at
                     FROM price_list_prices f
                     LEFT JOIN variants v ON v.id = f.variant_id
                     LEFT JOIN products p ON p.id = v.product_id
                     WHERE f.price_list_id = $1 AND ($2::uuid IS NULL OR f.variant_id < $2)
                       AND ($4::uuid IS NULL OR p.id = $4)
                       AND ($5::uuid IS NULL OR EXISTS (SELECT 1 FROM category_products cp
                            WHERE cp.category_id = $5 AND cp.product_id = p.id))
                       AND ($6::text IS NULL
                            OR strpos(lower(p.title), lower($6)) > 0
                            OR strpos(lower(p.handle), lower($6)) > 0
                            OR strpos(lower(v.sku), lower($6)) > 0)
                     ORDER BY f.variant_id DESC LIMIT $3",
                &[
                    (&list.as_uuid(), Type::UUID),
                    (&after, Type::UUID),
                    (&fetch, Type::INT8),
                    (&product, Type::UUID),
                    (&category, Type::UUID),
                    (&q, Type::TEXT),
                ],
            )
            .await
            .map_err(repo_error)?;
        let prices = rows
            .iter()
            .map(fixed_price_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Page::from_overfetch(prices, page.limit))
    }

    async fn insert(&self, price_list: &mut PriceList) -> Result<(), RepoError> {
        stamp_insert(price_list, self.now());
        let s = price_list.snapshot();
        let events = price_list.take_events();
        let stamp = self.event_stamp();
        let version = Version::NEW.next();
        in_transaction!(self, |tx| {
            insert_price_list(tx, &s).await?;
            insert_events(tx, &events, Some(version), stamp).await
        })?;
        price_list.set_version(version);
        Ok(())
    }

    async fn update(&self, price_list: &mut PriceList) -> Result<(), RepoError> {
        stamp_update(price_list, self.now());
        let s = price_list.snapshot();
        let events = price_list.take_events();
        let stamp = self.event_stamp();
        let version = in_transaction!(self, |tx| {
            let version = update_price_list(tx, &s).await?;
            insert_events(tx, &events, Some(version), stamp).await?;
            Ok(version)
        })?;
        price_list.set_version(version);
        Ok(())
    }

    async fn set_fixed_prices(
        &self,
        list: PriceListId,
        prices: &[(VariantId, FixedPrice)],
    ) -> Result<(), RepoError> {
        in_transaction!(self, |tx| set_fixed_prices(tx, list, prices).await)
    }

    async fn remove_fixed_prices(
        &self,
        list: PriceListId,
        variants: &[VariantId],
    ) -> Result<(), RepoError> {
        in_transaction!(self, |tx| remove_fixed_prices(tx, list, variants).await)
    }

    async fn lists_pricing(&self, variant: VariantId) -> Result<Vec<PriceListId>, RepoError> {
        let mut conn = self.conn().await;
        conn.db()
            .query_typed(
                "SELECT DISTINCT price_list_id FROM price_list_prices WHERE variant_id = $1
                 ORDER BY price_list_id",
                &[(&variant.as_uuid(), Type::UUID)],
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(|row| Ok(PriceListId::from_uuid(col(row, "price_list_id")?)))
            .collect()
    }

    async fn fixed_prices(
        &self,
        list: PriceListId,
        variants: &[VariantId],
    ) -> Result<HashMap<VariantId, FixedPrice>, RepoError> {
        if variants.is_empty() {
            return Ok(HashMap::new());
        }
        let ids: Vec<Uuid> = variants.iter().map(|v| v.as_uuid()).collect();
        let mut conn = self.conn().await;
        let rows = conn
            .db()
            .query_typed(
                "SELECT variant_id, amount, currency, compare_at FROM price_list_prices
                 WHERE price_list_id = $1 AND variant_id = ANY($2)",
                &[(&list.as_uuid(), Type::UUID), (&ids, Type::UUID_ARRAY)],
            )
            .await
            .map_err(repo_error)?;
        rows.iter().map(fixed_price_from_row).collect()
    }
}

/// `PriceListRepo::insert` inside the caller's transaction.
pub(crate) async fn insert_price_list(
    tx: &impl Queries,
    s: &PriceListSnapshot,
) -> Result<(), RepoError> {
    let (percent, applies_to, targets) = adjustment_to_db(s);
    tx.query_typed(
        &format!(
            "INSERT INTO price_lists (id, version, name, description, currency, kind, status,
                 starts_at, ends_at, compare_at, adjustment, applies_to, applies_to_ids)
             VALUES ($1, 1, $2, $3, $4, $5, $6, {}, {}, $9, $10, $11, $12)",
            ts(7),
            ts(8),
        ),
        &[
            (&s.id.as_uuid(), Type::UUID),
            (&s.name, Type::TEXT),
            (&s.description, Type::TEXT),
            (&s.currency.as_str(), Type::TEXT),
            (&s.kind.as_str(), Type::TEXT),
            (&s.status.as_str(), Type::TEXT),
            (&opt_micros(s.starts_at), Type::INT8),
            (&opt_micros(s.ends_at), Type::INT8),
            (&s.compare_at.as_str(), Type::TEXT),
            (&percent, Type::INT4),
            (&applies_to, Type::TEXT),
            (&targets, Type::UUID_ARRAY),
        ],
    )
    .await
    .map_err(repo_error)?;
    write_stamps(
        tx,
        "price_lists",
        "id",
        &s.id.as_uuid(),
        Type::UUID,
        s.stamps,
    )
    .await?;
    Ok(())
}

/// `PriceListRepo::update` inside the caller's transaction; the new version.
pub(crate) async fn update_price_list(
    tx: &impl Queries,
    s: &PriceListSnapshot,
) -> Result<Version, RepoError> {
    let (percent, applies_to, targets) = adjustment_to_db(s);
    let bumped = tx
        .query_typed_opt(
            &format!(
                "UPDATE price_lists SET version = version + 1, name = $3, description = $4,
                     currency = $5, kind = $6, status = $7, starts_at = {}, ends_at = {},
                     compare_at = $10, adjustment = $11, applies_to = $12, applies_to_ids = $13
                 WHERE id = $1 AND version = $2 RETURNING version",
                ts(8),
                ts(9),
            ),
            &[
                (&s.id.as_uuid(), Type::UUID),
                (&version_to_db(s.version)?, Type::INT8),
                (&s.name, Type::TEXT),
                (&s.description, Type::TEXT),
                (&s.currency.as_str(), Type::TEXT),
                (&s.kind.as_str(), Type::TEXT),
                (&s.status.as_str(), Type::TEXT),
                (&opt_micros(s.starts_at), Type::INT8),
                (&opt_micros(s.ends_at), Type::INT8),
                (&s.compare_at.as_str(), Type::TEXT),
                (&percent, Type::INT4),
                (&applies_to, Type::TEXT),
                (&targets, Type::UUID_ARRAY),
            ],
        )
        .await
        .map_err(repo_error)?;
    let row = bumped.ok_or(RepoError::StaleVersion)?;
    let written = version_from_db(col(&row, "version")?)?;
    write_stamps(
        tx,
        "price_lists",
        "id",
        &s.id.as_uuid(),
        Type::UUID,
        s.stamps,
    )
    .await?;
    Ok(written)
}

/// `PriceListRepo::set_fixed_prices` inside the caller's transaction.
pub(crate) async fn set_fixed_prices(
    tx: &impl Queries,
    list: PriceListId,
    prices: &[(VariantId, FixedPrice)],
) -> Result<(), RepoError> {
    let variants: Vec<Uuid> = prices.iter().map(|(v, _)| v.as_uuid()).collect();
    let amounts: Vec<i64> = prices
        .iter()
        .map(|(_, p)| p.price().money().amount())
        .collect();
    let currencies: Vec<&str> = prices
        .iter()
        .map(|(_, p)| p.price().money().currency().as_str())
        .collect();
    let compare_at: Vec<Option<i64>> = prices
        .iter()
        .map(|(_, p)| p.compare_at().map(|c| c.money().amount()))
        .collect();
    tx.query_typed(
        "INSERT INTO price_list_prices (price_list_id, variant_id, amount, currency, compare_at)
         SELECT $1, v, a, c, w
         FROM unnest($2::uuid[], $3::bigint[], $4::text[], $5::bigint[]) AS t (v, a, c, w)
         ON CONFLICT (price_list_id, variant_id)
         DO UPDATE SET amount = EXCLUDED.amount, currency = EXCLUDED.currency,
             compare_at = EXCLUDED.compare_at",
        &[
            (&list.as_uuid(), Type::UUID),
            (&variants, Type::UUID_ARRAY),
            (&amounts, Type::INT8_ARRAY),
            (&currencies, Type::TEXT_ARRAY),
            (&compare_at, Type::INT8_ARRAY),
        ],
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}

/// `PriceListRepo::remove_fixed_prices` inside the caller's transaction.
pub(crate) async fn remove_fixed_prices(
    tx: &impl Queries,
    list: PriceListId,
    variants: &[VariantId],
) -> Result<(), RepoError> {
    let ids: Vec<Uuid> = variants.iter().map(|v| v.as_uuid()).collect();
    tx.query_typed(
        "DELETE FROM price_list_prices WHERE price_list_id = $1 AND variant_id = ANY($2)",
        &[(&list.as_uuid(), Type::UUID), (&ids, Type::UUID_ARRAY)],
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}

/// An adjustment as its columns: the percentage (`NULL` without one), what it applies to
/// and the listed IDs.
fn adjustment_to_db(s: &PriceListSnapshot) -> (Option<i32>, &'static str, Vec<Uuid>) {
    match &s.adjustment {
        None => (None, "all", Vec::new()),
        Some(a) => {
            let targets = match a.applies_to() {
                AppliesTo::All => Vec::new(),
                AppliesTo::Products(p) => p.iter().map(|id| id.as_uuid()).collect(),
                AppliesTo::Categories(c) => c.iter().map(|id| id.as_uuid()).collect(),
            };
            (Some(a.percent().value()), a.applies_to().as_str(), targets)
        }
    }
}

fn price_list_from_row(row: &Row) -> Result<PriceList, RepoError> {
    let id: Uuid = col(row, "id")?;
    let adjustment: Option<i32> = col(row, "adjustment")?;
    let kind: String = col(row, "kind")?;
    let status: String = col(row, "status")?;
    let compare_at: String = col(row, "compare_at")?;
    PriceList::restore(PriceListSnapshot {
        id: PriceListId::from_uuid(id),
        version: version_from_db(col(row, "version")?)?,
        stamps: stamps_from_row(row)?,
        name: col(row, "name")?,
        description: col(row, "description")?,
        currency: currency_from_db(col(row, "currency")?)?,
        kind: PriceListKind::parse(&kind)
            .ok_or_else(|| corrupt(format!("price list kind {kind:?}")))?,
        status: PriceListStatus::parse(&status)
            .ok_or_else(|| corrupt(format!("price list status {status:?}")))?,
        starts_at: opt_ts(col(row, "starts_at")?),
        ends_at: opt_ts(col(row, "ends_at")?),
        compare_at: CompareAtMode::parse(&compare_at)
            .ok_or_else(|| corrupt(format!("price list compare-at mode {compare_at:?}")))?,
        adjustment: match adjustment {
            None => None,
            Some(percent) => {
                let percent = BasisPoints::new(percent).map_err(corrupt)?;
                let applies_to: String = col(row, "applies_to")?;
                let targets: Vec<Uuid> = col(row, "applies_to_ids")?;
                let applies_to = match applies_to.as_str() {
                    "all" => AppliesTo::All,
                    "products" => {
                        AppliesTo::Products(targets.into_iter().map(ProductId::from_uuid).collect())
                    }
                    "categories" => AppliesTo::Categories(
                        targets.into_iter().map(CategoryId::from_uuid).collect(),
                    ),
                    other => return Err(corrupt(format!("applies to {other:?}"))),
                };
                Some(Adjustment::new(percent, applies_to).map_err(corrupt)?)
            }
        },
    })
    .map_err(|e| corrupt(format!("price list {id}: {e}")))
}

/// A `price_list_prices` row (`variant_id, amount, currency, compare_at`).
fn fixed_price_from_row(row: &Row) -> Result<(VariantId, FixedPrice), RepoError> {
    let currency = currency_from_db(col(row, "currency")?)?;
    let price = |amount: i64| {
        hypha_core::Money::new(amount, currency)
            .and_then(Price::new)
            .map_err(corrupt)
    };
    let compare_at: Option<i64> = col(row, "compare_at")?;
    let fixed = FixedPrice::new(
        price(col(row, "amount")?)?,
        compare_at.map(price).transpose()?,
    )
    .map_err(corrupt)?;
    Ok((VariantId::from_uuid(col(row, "variant_id")?), fixed))
}
