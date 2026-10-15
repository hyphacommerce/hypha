use std::collections::{HashMap, HashSet};

use hypha_core::{
    BasisPoints, CatalogId, CountryCode, MarketId, Price, PriceListId, ProductId, Recorded,
    RepoError, VariantId, Version, Versioned, stamp_insert, stamp_update,
};
use hypha_markets::{
    Catalog, CatalogContext, CatalogRepo, CatalogSnapshot, Market, MarketRepo, MarketSnapshot,
    PriceList, PriceListRepo, PriceListSnapshot,
};
use tokio_postgres::{GenericClient, Row, types::Type};
use uuid::Uuid;

use crate::{
    columns::{
        col, currency_from_db, stamp_columns, stamps_from_row, version_from_db, version_to_db,
        write_stamps,
    },
    error::{corrupt, repo_error},
    outbox::insert_events,
    store::{PgConnection, PgStore, consistent_read, in_transaction},
};

// ---------- markets ----------

impl<C: PgConnection> MarketRepo for PgStore<C> {
    async fn get(&self, id: MarketId) -> Result<Option<Market>, RepoError> {
        let mut conn = self.conn().await;
        consistent_read!(conn.client(), |tx| {
            Ok(load_markets(tx, Some(id.as_uuid())).await?.pop())
        })
    }

    async fn list_all(&self) -> Result<Vec<Market>, RepoError> {
        let mut conn = self.conn().await;
        consistent_read!(conn.client(), |tx| load_markets(tx, None).await)
    }

    async fn get_by_country(&self, country: CountryCode) -> Result<Option<Market>, RepoError> {
        let mut conn = self.conn().await;
        consistent_read!(conn.client(), |tx| {
            let row = tx
                .query_typed_opt(
                    "SELECT market_id FROM market_countries WHERE country = $1",
                    &[(&country.as_str(), Type::TEXT)],
                )
                .await
                .map_err(repo_error)?;
            match row {
                Some(row) => Ok(load_markets(tx, Some(col(&row, "market_id")?)).await?.pop()),
                None => Ok(None),
            }
        })
    }

    async fn insert(&self, market: &mut Market) -> Result<(), RepoError> {
        stamp_insert(market, self.now());
        let s = market.snapshot();
        let events = market.take_events();
        let actor = self.actor();
        let version = Version::NEW.next();
        in_transaction!(self, |tx| {
            insert_market(tx, &s).await?;
            insert_events(tx, &events, Some(version), actor).await
        })?;
        market.set_version(version);
        Ok(())
    }

    async fn update(&self, market: &mut Market) -> Result<(), RepoError> {
        stamp_update(market, self.now());
        let s = market.snapshot();
        let events = market.take_events();
        let actor = self.actor();
        let version = in_transaction!(self, |tx| {
            let version = update_market(tx, &s).await?;
            insert_events(tx, &events, Some(version), actor).await?;
            Ok(version)
        })?;
        market.set_version(version);
        Ok(())
    }
}

/// `MarketRepo::insert` inside the caller's transaction: the row, then its countries.
pub(crate) async fn insert_market(
    tx: &impl GenericClient,
    s: &MarketSnapshot,
) -> Result<(), RepoError> {
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
    tx: &impl GenericClient,
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

async fn write_countries(tx: &impl GenericClient, s: &MarketSnapshot) -> Result<(), RepoError> {
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
async fn load_markets(
    client: &impl GenericClient,
    id: Option<Uuid>,
) -> Result<Vec<Market>, RepoError> {
    let filter = [(
        &id as &(dyn tokio_postgres::types::ToSql + Sync),
        Type::UUID,
    )];
    let rows = client
        .query_typed(
            &format!(
                "SELECT id, version, name, currency, prices_include_tax, {} FROM markets
                 WHERE $1::uuid IS NULL OR id = $1",
                stamp_columns(""),
            ),
            &filter,
        )
        .await
        .map_err(repo_error)?;
    let mut countries: HashMap<Uuid, Vec<CountryCode>> = HashMap::new();
    for row in client
        .query_typed(
            "SELECT market_id, country FROM market_countries
             WHERE $1::uuid IS NULL OR market_id = $1 ORDER BY market_id, position",
            &filter,
        )
        .await
        .map_err(repo_error)?
    {
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

fn catalog_from_row(row: &Row) -> Result<Catalog, RepoError> {
    let kind: String = col(row, "context_kind")?;
    let context = match kind.as_str() {
        "market" => CatalogContext::Market(MarketId::from_uuid(col(row, "context_id")?)),
        other => return Err(corrupt(format!("catalog context kind {other:?}"))),
    };
    let price_list: Option<Uuid> = col(row, "price_list_id")?;
    Ok(Catalog::restore(CatalogSnapshot {
        id: CatalogId::from_uuid(col(row, "id")?),
        version: version_from_db(col(row, "version")?)?,
        stamps: stamps_from_row(row)?,
        context,
        price_list: price_list.map(PriceListId::from_uuid),
    }))
}

/// `catalogs`' columns, as `catalog_from_row` reads them.
fn catalog_columns() -> String {
    format!(
        "id, version, context_kind, context_id, price_list_id, {}",
        stamp_columns("")
    )
}

impl<C: PgConnection> CatalogRepo for PgStore<C> {
    async fn get(&self, id: CatalogId) -> Result<Option<Catalog>, RepoError> {
        let mut conn = self.conn().await;
        let row = conn
            .client()
            .query_typed_opt(
                &format!("SELECT {} FROM catalogs WHERE id = $1", catalog_columns()),
                &[(&id.as_uuid(), Type::UUID)],
            )
            .await
            .map_err(repo_error)?;
        row.as_ref().map(catalog_from_row).transpose()
    }

    async fn get_by_context(&self, context: CatalogContext) -> Result<Option<Catalog>, RepoError> {
        let (kind, context_id) = context_to_db(context);
        let mut conn = self.conn().await;
        let row = conn
            .client()
            .query_typed_opt(
                &format!(
                    "SELECT {} FROM catalogs WHERE context_kind = $1 AND context_id = $2",
                    catalog_columns(),
                ),
                &[(&kind, Type::TEXT), (&context_id, Type::UUID)],
            )
            .await
            .map_err(repo_error)?;
        row.as_ref().map(catalog_from_row).transpose()
    }

    async fn insert(&self, catalog: &mut Catalog) -> Result<(), RepoError> {
        stamp_insert(catalog, self.now());
        let s = catalog.snapshot();
        let events = catalog.take_events();
        let actor = self.actor();
        let version = Version::NEW.next();
        in_transaction!(self, |tx| {
            insert_catalog(tx, &s).await?;
            insert_events(tx, &events, Some(version), actor).await
        })?;
        catalog.set_version(version);
        Ok(())
    }

    async fn update(&self, catalog: &mut Catalog) -> Result<(), RepoError> {
        stamp_update(catalog, self.now());
        let s = catalog.snapshot();
        let events = catalog.take_events();
        let actor = self.actor();
        let version = in_transaction!(self, |tx| {
            let version = update_catalog(tx, &s).await?;
            insert_events(tx, &events, Some(version), actor).await?;
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
            .client()
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

/// `CatalogRepo::insert` inside the caller's transaction.
pub(crate) async fn insert_catalog(
    tx: &impl GenericClient,
    s: &CatalogSnapshot,
) -> Result<(), RepoError> {
    let (kind, context_id) = context_to_db(s.context);
    tx.query_typed(
        "INSERT INTO catalogs (id, version, context_kind, context_id, price_list_id)
         VALUES ($1, 1, $2, $3, $4)",
        &[
            (&s.id.as_uuid(), Type::UUID),
            (&kind, Type::TEXT),
            (&context_id, Type::UUID),
            (&s.price_list.map(|p| p.as_uuid()), Type::UUID),
        ],
    )
    .await
    .map_err(repo_error)?;
    let written = ();
    write_stamps(tx, "catalogs", "id", &s.id.as_uuid(), Type::UUID, s.stamps).await?;
    Ok(written)
}

/// `CatalogRepo::update` inside the caller's transaction; the new version. Membership
/// rows are untouched.
pub(crate) async fn update_catalog(
    tx: &impl GenericClient,
    s: &CatalogSnapshot,
) -> Result<Version, RepoError> {
    let bumped = tx
        .query_typed_opt(
            "UPDATE catalogs SET version = version + 1, price_list_id = $3
             WHERE id = $1 AND version = $2 RETURNING version",
            &[
                (&s.id.as_uuid(), Type::UUID),
                (&version_to_db(s.version)?, Type::INT8),
                (&s.price_list.map(|p| p.as_uuid()), Type::UUID),
            ],
        )
        .await
        .map_err(repo_error)?;
    let row = bumped.ok_or(RepoError::StaleVersion)?;
    let written = version_from_db(col(&row, "version")?)?;
    write_stamps(tx, "catalogs", "id", &s.id.as_uuid(), Type::UUID, s.stamps).await?;
    Ok(written)
}

/// `CatalogRepo::add_products` inside the caller's transaction.
pub(crate) async fn add_catalog_products(
    tx: &impl GenericClient,
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
    tx: &impl GenericClient,
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

impl<C: PgConnection> PriceListRepo for PgStore<C> {
    async fn get(&self, id: PriceListId) -> Result<Option<PriceList>, RepoError> {
        let mut conn = self.conn().await;
        let row = conn
            .client()
            .query_typed_opt(
                &format!(
                    "SELECT id, version, currency, adjustment, {} FROM price_lists WHERE id = $1",
                    stamp_columns(""),
                ),
                &[(&id.as_uuid(), Type::UUID)],
            )
            .await
            .map_err(repo_error)?;
        let Some(row) = row else { return Ok(None) };
        let adjustment: i32 = col(&row, "adjustment")?;
        Ok(Some(PriceList::restore(PriceListSnapshot {
            id: PriceListId::from_uuid(col(&row, "id")?),
            version: version_from_db(col(&row, "version")?)?,
            stamps: stamps_from_row(&row)?,
            currency: currency_from_db(col(&row, "currency")?)?,
            adjustment: BasisPoints::new(adjustment).map_err(corrupt)?,
        })))
    }

    async fn insert(&self, price_list: &mut PriceList) -> Result<(), RepoError> {
        stamp_insert(price_list, self.now());
        let s = price_list.snapshot();
        let events = price_list.take_events();
        let actor = self.actor();
        let version = Version::NEW.next();
        in_transaction!(self, |tx| {
            insert_price_list(tx, &s).await?;
            insert_events(tx, &events, Some(version), actor).await
        })?;
        price_list.set_version(version);
        Ok(())
    }

    async fn update(&self, price_list: &mut PriceList) -> Result<(), RepoError> {
        stamp_update(price_list, self.now());
        let s = price_list.snapshot();
        let events = price_list.take_events();
        let actor = self.actor();
        let version = in_transaction!(self, |tx| {
            let version = update_price_list(tx, &s).await?;
            insert_events(tx, &events, Some(version), actor).await?;
            Ok(version)
        })?;
        price_list.set_version(version);
        Ok(())
    }

    async fn set_fixed_prices(
        &self,
        list: PriceListId,
        prices: &[(VariantId, Price)],
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

    async fn fixed_prices(
        &self,
        list: PriceListId,
        variants: &[VariantId],
    ) -> Result<HashMap<VariantId, Price>, RepoError> {
        if variants.is_empty() {
            return Ok(HashMap::new());
        }
        let ids: Vec<Uuid> = variants.iter().map(|v| v.as_uuid()).collect();
        let mut conn = self.conn().await;
        let rows = conn
            .client()
            .query_typed(
                "SELECT variant_id, amount, currency FROM price_list_prices
                 WHERE price_list_id = $1 AND variant_id = ANY($2)",
                &[(&list.as_uuid(), Type::UUID), (&ids, Type::UUID_ARRAY)],
            )
            .await
            .map_err(repo_error)?;
        rows.iter()
            .map(|row| {
                let money = hypha_core::Money::new(
                    col(row, "amount")?,
                    currency_from_db(col(row, "currency")?)?,
                )
                .map_err(corrupt)?;
                let price = Price::new(money).map_err(corrupt)?;
                Ok((VariantId::from_uuid(col(row, "variant_id")?), price))
            })
            .collect()
    }
}

/// `PriceListRepo::insert` inside the caller's transaction.
pub(crate) async fn insert_price_list(
    tx: &impl GenericClient,
    s: &PriceListSnapshot,
) -> Result<(), RepoError> {
    tx.query_typed(
        "INSERT INTO price_lists (id, version, currency, adjustment) VALUES ($1, 1, $2, $3)",
        &[
            (&s.id.as_uuid(), Type::UUID),
            (&s.currency.as_str(), Type::TEXT),
            (&s.adjustment.value(), Type::INT4),
        ],
    )
    .await
    .map_err(repo_error)?;
    let written = ();
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

/// `PriceListRepo::update` inside the caller's transaction; the new version.
pub(crate) async fn update_price_list(
    tx: &impl GenericClient,
    s: &PriceListSnapshot,
) -> Result<Version, RepoError> {
    let bumped = tx
        .query_typed_opt(
            "UPDATE price_lists SET version = version + 1, currency = $3, adjustment = $4
             WHERE id = $1 AND version = $2 RETURNING version",
            &[
                (&s.id.as_uuid(), Type::UUID),
                (&version_to_db(s.version)?, Type::INT8),
                (&s.currency.as_str(), Type::TEXT),
                (&s.adjustment.value(), Type::INT4),
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
    tx: &impl GenericClient,
    list: PriceListId,
    prices: &[(VariantId, Price)],
) -> Result<(), RepoError> {
    let variants: Vec<Uuid> = prices.iter().map(|(v, _)| v.as_uuid()).collect();
    let amounts: Vec<i64> = prices.iter().map(|(_, p)| p.money().amount()).collect();
    let currencies: Vec<&str> = prices
        .iter()
        .map(|(_, p)| p.money().currency().as_str())
        .collect();
    tx.query_typed(
        "INSERT INTO price_list_prices (price_list_id, variant_id, amount, currency)
         SELECT $1, v, a, c FROM unnest($2::uuid[], $3::bigint[], $4::text[]) AS t (v, a, c)
         ON CONFLICT (price_list_id, variant_id)
         DO UPDATE SET amount = EXCLUDED.amount, currency = EXCLUDED.currency",
        &[
            (&list.as_uuid(), Type::UUID),
            (&variants, Type::UUID_ARRAY),
            (&amounts, Type::INT8_ARRAY),
            (&currencies, Type::TEXT_ARRAY),
        ],
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}

/// `PriceListRepo::remove_fixed_prices` inside the caller's transaction.
pub(crate) async fn remove_fixed_prices(
    tx: &impl GenericClient,
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
