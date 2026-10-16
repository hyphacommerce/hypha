//! Search in Postgres (design note 0019, SE9): the built-in engine, the queue of what to
//! rebuild, and the merchant's settings.
//!
//! The engine answers exactly as `hypha_search_index::reference::search` (the
//! `search_index` contract checks it): terms, scores and facet values are computed in
//! Rust when a document is written (`SearchDocument::terms`, `::facet_values`) and only
//! compared in SQL, titles sort by a lowercased copy made in Rust, and text compares
//! byte by byte (`COLLATE "C"`), so the database's locale changes nothing. `tsv` only
//! narrows a text search through its GIN index; whether a document matches is decided
//! on the terms.

use std::collections::{BTreeMap, BTreeSet};

use hypha_core::{
    AssetId, MarketId, Money, ProductId, RepoError, Timestamp, VariantId, Version, Versioned,
    stamp_insert, stamp_update,
};
use hypha_search_index::{
    FacetCounts, FacetKey, FacetValue, Group, Hit, IndexError, MAX_FACET_VALUES, Mark, PriceRange,
    SearchDocument, SearchIndex, SearchQuery, SearchQueue, SearchResults, SearchSettings,
    SearchSettingsRepo, SearchSettingsSnapshot, Sort, Target, reference,
};
use tokio_postgres::{
    GenericClient, Row,
    types::{ToSql, Type},
};
use uuid::Uuid;

use crate::{
    columns::{
        col, currency_from_db, stamp_columns, stamps_from_row, ts, version_from_db, version_to_db,
        write_stamps,
    },
    error::{corrupt, repo_error},
    outbox::insert_events,
    store::{PgConnection, PgStore, consistent_read, in_transaction},
};

/// Separates a facet's key from its value in `facets` (`option.color` 0x1F `Red`).
const SEPARATOR: char = '\u{1f}';

// ---------- the engine ----------

impl<C: PgConnection> SearchIndex for PgStore<C> {
    async fn replace(
        &self,
        market: MarketId,
        product: ProductId,
        documents: &[SearchDocument],
    ) -> Result<(), IndexError> {
        in_transaction!(self, |tx| {
            tx.query_typed(
                "DELETE FROM search_documents WHERE market_id = $1 AND product_id = $2",
                &[
                    (&market.as_uuid(), Type::UUID),
                    (&product.as_uuid(), Type::UUID),
                ],
            )
            .await
            .map_err(repo_error)?;
            for document in documents {
                insert_document(tx, document).await?;
            }
            Ok(())
        })?;
        Ok(())
    }

    async fn search(&self, query: &SearchQuery) -> Result<SearchResults, IndexError> {
        let mut conn = self.conn().await;
        let results = consistent_read!(conn.client(), |tx| run_search(tx, query).await)?;
        Ok(results)
    }

    async fn option_names(&self, market: MarketId) -> Result<Vec<String>, IndexError> {
        let mut conn = self.conn().await;
        let rows = conn
            .client()
            .query_typed(
                "SELECT split_part(fv, chr(31), 1) AS key, count(*) AS n
                 FROM search_documents d, unnest(d.facets) fv
                 WHERE d.market_id = $1 AND starts_with(fv, 'option.')
                 GROUP BY 1",
                &[(&market.as_uuid(), Type::UUID)],
            )
            .await
            .map_err(repo_error)?;
        let mut names = rows
            .iter()
            .map(|row| {
                let key: String = col(row, "key")?;
                let n: i64 = col(row, "n")?;
                match key.parse::<FacetKey>() {
                    Ok(FacetKey::Option(name)) => Ok((name, n)),
                    _ => Err(corrupt(format!("search facet key {key:?}"))),
                }
            })
            .collect::<Result<Vec<_>, RepoError>>()?;
        names.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        Ok(names.into_iter().map(|(name, _)| name).collect())
    }

    async fn count(&self) -> Result<u64, IndexError> {
        let mut conn = self.conn().await;
        let row = conn
            .client()
            .query_typed_one("SELECT count(*) AS n FROM search_documents", &[])
            .await
            .map_err(repo_error)?;
        let n: i64 = col(&row, "n")?;
        Ok(u64::try_from(n).map_err(corrupt)?)
    }
}

async fn insert_document(tx: &impl GenericClient, d: &SearchDocument) -> Result<(), RepoError> {
    let terms = d.terms();
    let all_terms = terms.all().join(" ");
    let (option_names, option_values): (Vec<&str>, Vec<&str>) = d
        .options
        .iter()
        .map(|(n, v)| (n.as_str(), v.as_str()))
        .unzip();
    let facets: Vec<String> = d
        .facet_values()
        .into_iter()
        .map(|(key, value)| format!("{key}{SEPARATOR}{value}"))
        .collect();
    let position = i32::try_from(d.position).map_err(corrupt)?;
    tx.query_typed(
        "INSERT INTO search_documents (
             market_id, product_id, variant_id, listing, handle, title, title_sort,
             variant_title, description, vendor, product_type, tags, sku, option_names,
             option_values, currency, price, product_min, product_max, listing_min,
             listing_max, available, image_id, created_us, position, terms_a, terms_b,
             terms_d, tsv, facets)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17,
                 $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28,
                 to_tsvector('simple', $29), $30)",
        &[
            (&d.market.as_uuid(), Type::UUID),
            (&d.product.as_uuid(), Type::UUID),
            (&d.variant.as_uuid(), Type::UUID),
            (&d.listing, Type::TEXT),
            (&d.handle, Type::TEXT),
            (&d.title, Type::TEXT),
            (&d.title.to_lowercase(), Type::TEXT),
            (&d.variant_title, Type::TEXT),
            (&d.description, Type::TEXT),
            (&d.vendor, Type::TEXT),
            (&d.product_type, Type::TEXT),
            (&d.tags, Type::TEXT_ARRAY),
            (&d.sku, Type::TEXT),
            (&option_names, Type::TEXT_ARRAY),
            (&option_values, Type::TEXT_ARRAY),
            (&d.price.currency().as_str(), Type::TEXT),
            (&d.price.amount(), Type::INT8),
            (&d.product_price.min.amount(), Type::INT8),
            (&d.product_price.max.amount(), Type::INT8),
            (&d.listing_price.min.amount(), Type::INT8),
            (&d.listing_price.max.amount(), Type::INT8),
            (&d.available, Type::BOOL),
            (&d.image.map(|a| a.as_uuid()), Type::UUID),
            (&d.created_at.unix_micros(), Type::INT8),
            (&position, Type::INT4),
            (&terms.a, Type::TEXT_ARRAY),
            (&terms.b, Type::TEXT_ARRAY),
            (&terms.d, Type::TEXT_ARRAY),
            (&all_terms, Type::TEXT),
            (&facets, Type::TEXT_ARRAY),
        ],
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}

/// Query parameters, numbered as they're added.
#[derive(Default)]
struct Params {
    values: Vec<(Box<dyn ToSql + Sync + Send>, Type)>,
}

impl Params {
    /// Adds `value`; its placeholder (`$3`).
    fn add<T: ToSql + Sync + Send + 'static>(&mut self, value: T, ty: Type) -> String {
        self.values.push((Box::new(value), ty));
        format!("${}", self.values.len())
    }

    fn as_slice(&self) -> Vec<(&(dyn ToSql + Sync), Type)> {
        self.values
            .iter()
            .map(|(v, t)| (v.as_ref() as &(dyn ToSql + Sync), t.clone()))
            .collect()
    }
}

/// What a condition leaves out, for disjunctive facets and the price range.
#[derive(Clone, Copy, PartialEq)]
enum Except<'a> {
    Nothing,
    Price,
    Facet(&'a FacetKey),
}

/// The query's conditions on `d`, as `reference::search` applies them.
fn conditions(q: &SearchQuery, params: &mut Params, except: Except<'_>) -> String {
    let mut sql = vec![format!(
        "d.market_id = {}",
        params.add(q.market.as_uuid(), Type::UUID)
    )];
    if !q.terms.is_empty() {
        // The index narrows (every term a prefix of a lexeme); the terms decide.
        let tsquery: Vec<String> = q.terms.iter().map(|t| format!("{t}:*")).collect();
        sql.push(format!(
            "d.tsv @@ to_tsquery('simple', {})",
            params.add(tsquery.join(" & "), Type::TEXT)
        ));
        sql.push(format!(
            "NOT EXISTS (SELECT 1 FROM unnest({}::text[]) q WHERE NOT EXISTS (
                 SELECT 1 FROM unnest(d.terms_a || d.terms_b || d.terms_d) t
                 WHERE starts_with(t, q)))",
            params.add(q.terms.clone(), Type::TEXT_ARRAY)
        ));
    }
    if let Some(available) = q.available {
        sql.push(format!(
            "d.available = {}",
            params.add(available, Type::BOOL)
        ));
    }
    if except != Except::Price {
        if let Some(min) = q.price.min {
            sql.push(format!("d.price >= {}", params.add(min, Type::INT8)));
        }
        if let Some(max) = q.price.max {
            sql.push(format!("d.price <= {}", params.add(max, Type::INT8)));
        }
    }
    for filter in &q.filters {
        if except == Except::Facet(&filter.key) {
            continue;
        }
        let entries: Vec<String> = filter
            .values
            .iter()
            .map(|v| format!("{}{SEPARATOR}{v}", filter.key))
            .collect();
        sql.push(format!(
            "d.facets && {}::text[]",
            params.add(entries, Type::TEXT_ARRAY)
        ));
    }
    sql.join(" AND ")
}

fn group_expr(group: Group) -> &'static str {
    match group {
        Group::Listing => "d.listing",
        Group::Product => "d.product_id::text",
        Group::Variant => "d.variant_id::text",
    }
}

/// `reference::score` in SQL: per query term, 10, 4 or 1 for the best field it
/// prefixes a term of, summed.
fn score_expr(q: &SearchQuery, params: &mut Params) -> String {
    if q.terms.is_empty() {
        return "0".to_string();
    }
    let has =
        |field: &str| format!("EXISTS (SELECT 1 FROM unnest(d.{field}) t WHERE starts_with(t, q))");
    format!(
        "(SELECT coalesce(sum(CASE WHEN {} THEN 10 WHEN {} THEN 4 WHEN {} THEN 1 ELSE 0 END), 0)
          FROM unnest({}::text[]) q)",
        has("terms_a"),
        has("terms_b"),
        has("terms_d"),
        params.add(q.terms.clone(), Type::TEXT_ARRAY)
    )
}

async fn run_search(tx: &impl GenericClient, q: &SearchQuery) -> Result<SearchResults, RepoError> {
    let sort = reference::effective_sort(q);
    let group = group_expr(q.group);

    // Hits: matches with their group and score; each group's representative; the page.
    let mut params = Params::default();
    let matched = conditions(q, &mut params, Except::Nothing);
    let score = score_expr(q, &mut params);
    let within = match sort {
        Sort::PriceAsc => "price ASC, product_id, position",
        Sort::PriceDesc => "price DESC, product_id, position",
        Sort::Relevance => "score DESC, product_id, position",
        Sort::Newest | Sort::Title => "product_id, position",
    };
    let by_sort = match sort {
        Sort::PriceAsc => "price ASC, ",
        Sort::PriceDesc => "price DESC, ",
        Sort::Relevance => "score DESC, ",
        Sort::Title => "title_sort COLLATE \"C\" ASC, ",
        Sort::Newest => "",
    };
    let offset = params.add(i64::from(q.offset), Type::INT8);
    let limit = params.add(i64::from(q.limit), Type::INT8);
    let hits_sql = format!(
        "WITH m AS (
             SELECT d.*, {group} AS g, {score} AS score
             FROM search_documents d WHERE {matched}
         ), best AS (
             SELECT DISTINCT ON (g) m.*, count(*) OVER (PARTITION BY g) AS matched
             FROM m ORDER BY g, {within}
         )
         SELECT * FROM best
         ORDER BY {by_sort}created_us DESC, product_id DESC, listing COLLATE \"C\" ASC,
                  position ASC
         OFFSET {offset} LIMIT {limit}"
    );
    let hits = tx
        .query_typed(&hits_sql, &params.as_slice())
        .await
        .map_err(repo_error)?
        .iter()
        .map(|row| {
            Ok(Hit {
                document: document_from_row(row)?,
                matched: u32::try_from(col::<i64>(row, "matched")?).map_err(corrupt)?,
            })
        })
        .collect::<Result<Vec<_>, RepoError>>()?;

    // The number of groups, and the price range ignoring the price bounds.
    let mut params = Params::default();
    let matched = conditions(q, &mut params, Except::Nothing);
    let unbounded = conditions(q, &mut params, Except::Price);
    let row = tx
        .query_typed_one(
            &format!(
                "SELECT (SELECT count(DISTINCT {group}) FROM search_documents d WHERE {matched})
                            AS total,
                        min(d.price) AS low, max(d.price) AS high, min(d.currency) AS currency
                 FROM search_documents d WHERE {unbounded}"
            ),
            &params.as_slice(),
        )
        .await
        .map_err(repo_error)?;
    let total = u64::try_from(col::<i64>(&row, "total")?).map_err(corrupt)?;
    let price = match (
        col::<Option<i64>>(&row, "low")?,
        col::<Option<i64>>(&row, "high")?,
        col::<Option<String>>(&row, "currency")?,
    ) {
        (Some(low), Some(high), Some(currency)) => {
            let currency = currency_from_db(&currency)?;
            let money = |n| Money::new(n, currency).map_err(corrupt);
            Some((money(low)?, money(high)?))
        }
        _ => None,
    };

    Ok(SearchResults {
        hits,
        total,
        facets: facet_counts(tx, q, group).await?,
        price,
    })
}

/// Each asked-for facet's values, counting groups and ignoring the facet's own filter,
/// in one statement.
async fn facet_counts(
    tx: &impl GenericClient,
    q: &SearchQuery,
    group: &str,
) -> Result<Vec<FacetCounts>, RepoError> {
    if q.facets.is_empty() {
        return Ok(Vec::new());
    }
    let mut params = Params::default();
    let branches: Vec<String> = q
        .facets
        .iter()
        .map(|key| {
            let conds = conditions(q, &mut params, Except::Facet(key));
            let prefix = params.add(format!("{key}{SEPARATOR}"), Type::TEXT);
            format!(
                "SELECT fv, count(DISTINCT {group}) AS n
                 FROM search_documents d, unnest(d.facets) fv
                 WHERE {conds} AND starts_with(fv, {prefix})
                 GROUP BY fv"
            )
        })
        .collect();
    let rows = tx
        .query_typed(&branches.join(" UNION ALL "), &params.as_slice())
        .await
        .map_err(repo_error)?;
    let mut by_key: BTreeMap<String, Vec<FacetValue>> = BTreeMap::new();
    for row in &rows {
        let entry: String = col(row, "fv")?;
        let (key, value) = entry
            .split_once(SEPARATOR)
            .ok_or_else(|| corrupt(format!("search facet {entry:?}")))?;
        by_key.entry(key.to_string()).or_default().push(FacetValue {
            value: value.to_string(),
            count: u64::try_from(col::<i64>(row, "n")?).map_err(corrupt)?,
        });
    }
    Ok(q.facets
        .iter()
        .map(|key| {
            let mut values = by_key.remove(&key.to_string()).unwrap_or_default();
            values.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.value.cmp(&b.value)));
            values.truncate(MAX_FACET_VALUES);
            FacetCounts {
                key: key.clone(),
                values,
            }
        })
        .collect())
}

fn document_from_row(row: &Row) -> Result<SearchDocument, RepoError> {
    let currency = currency_from_db(&col::<String>(row, "currency")?)?;
    let money = |column: &str| -> Result<Money, RepoError> {
        Money::new(col(row, column)?, currency).map_err(corrupt)
    };
    let names: Vec<String> = col(row, "option_names")?;
    let values: Vec<String> = col(row, "option_values")?;
    Ok(SearchDocument {
        market: MarketId::from_uuid(col(row, "market_id")?),
        product: ProductId::from_uuid(col(row, "product_id")?),
        variant: VariantId::from_uuid(col(row, "variant_id")?),
        listing: col(row, "listing")?,
        handle: col(row, "handle")?,
        title: col(row, "title")?,
        variant_title: col(row, "variant_title")?,
        description: col(row, "description")?,
        vendor: col(row, "vendor")?,
        product_type: col(row, "product_type")?,
        tags: col(row, "tags")?,
        sku: col(row, "sku")?,
        options: names.into_iter().zip(values).collect(),
        price: money("price")?,
        product_price: PriceRange {
            min: money("product_min")?,
            max: money("product_max")?,
        },
        listing_price: PriceRange {
            min: money("listing_min")?,
            max: money("listing_max")?,
        },
        available: col(row, "available")?,
        image: col::<Option<Uuid>>(row, "image_id")?.map(AssetId::from_uuid),
        created_at: Timestamp::from_unix_micros(col(row, "created_us")?),
        position: u32::try_from(col::<i32>(row, "position")?).map_err(corrupt)?,
    })
}

// ---------- the queue ----------

fn target_to_db(target: Target) -> String {
    match target {
        Target::Product(id) => format!("product:{}", id.as_uuid()),
        Target::Variant(id) => format!("variant:{}", id.as_uuid()),
        Target::All => "all".to_string(),
    }
}

fn target_from_db(text: &str) -> Result<Target, RepoError> {
    let bad = || corrupt(format!("search target {text:?}"));
    match text.split_once(':') {
        None if text == "all" => Ok(Target::All),
        Some(("product", id)) => Ok(Target::Product(ProductId::from_uuid(
            id.parse().map_err(|_| bad())?,
        ))),
        Some(("variant", id)) => Ok(Target::Variant(VariantId::from_uuid(
            id.parse().map_err(|_| bad())?,
        ))),
        _ => Err(bad()),
    }
}

impl<C: PgConnection> SearchQueue for PgStore<C> {
    async fn mark(&self, targets: &[Target]) -> Result<(), RepoError> {
        // One row may be updated only once per statement: mark each target once.
        let mut seen = BTreeSet::new();
        let targets: Vec<String> = targets
            .iter()
            .filter(|t| seen.insert(**t))
            .map(|t| target_to_db(*t))
            .collect();
        if targets.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn().await;
        conn.client()
            .query_typed(
                "INSERT INTO search_queue (target, generation)
                 SELECT t, 1 FROM unnest($1::text[]) WITH ORDINALITY AS m (t, n) ORDER BY n
                 ON CONFLICT (target) DO UPDATE SET generation = search_queue.generation + 1",
                &[(&targets, Type::TEXT_ARRAY)],
            )
            .await
            .map_err(repo_error)?;
        Ok(())
    }

    async fn lease(
        &self,
        now: Timestamp,
        until: Timestamp,
        limit: u32,
    ) -> Result<Vec<Mark>, RepoError> {
        let mut conn = self.conn().await;
        let rows = conn
            .client()
            .query_typed(
                &format!(
                    "UPDATE search_queue q SET leased_until = {}
                     FROM (SELECT target FROM search_queue
                           WHERE leased_until IS NULL OR leased_until <= {}
                           ORDER BY seq LIMIT $3 FOR UPDATE SKIP LOCKED) free
                     WHERE q.target = free.target
                     RETURNING q.target, q.generation, q.seq",
                    ts(2),
                    ts(1)
                ),
                &[
                    (&now.unix_micros(), Type::INT8),
                    (&until.unix_micros(), Type::INT8),
                    (&i64::from(limit), Type::INT8),
                ],
            )
            .await
            .map_err(repo_error)?;
        let mut marks = rows
            .iter()
            .map(|row| {
                let seq: i64 = col(row, "seq")?;
                let target: String = col(row, "target")?;
                Ok((
                    seq,
                    Mark {
                        target: target_from_db(&target)?,
                        generation: col(row, "generation")?,
                    },
                ))
            })
            .collect::<Result<Vec<_>, RepoError>>()?;
        marks.sort_by_key(|(seq, _)| *seq);
        Ok(marks.into_iter().map(|(_, mark)| mark).collect())
    }

    async fn finish(&self, marks: &[Mark]) -> Result<(), RepoError> {
        if marks.is_empty() {
            return Ok(());
        }
        let targets: Vec<String> = marks.iter().map(|m| target_to_db(m.target)).collect();
        let generations: Vec<i64> = marks.iter().map(|m| m.generation).collect();
        in_transaction!(self, |tx| {
            tx.query_typed(
                "DELETE FROM search_queue q
                 USING unnest($1::text[], $2::bigint[]) AS f (target, generation)
                 WHERE q.target = f.target AND q.generation = f.generation",
                &[
                    (&targets, Type::TEXT_ARRAY),
                    (&generations, Type::INT8_ARRAY),
                ],
            )
            .await
            .map_err(repo_error)?;
            // Marked again meanwhile: free for the next run at once.
            tx.query_typed(
                "UPDATE search_queue SET leased_until = NULL WHERE target = ANY($1)",
                &[(&targets, Type::TEXT_ARRAY)],
            )
            .await
            .map_err(repo_error)?;
            Ok(())
        })
    }

    async fn queued(&self) -> Result<u64, RepoError> {
        let mut conn = self.conn().await;
        let row = conn
            .client()
            .query_typed_one("SELECT count(*) AS n FROM search_queue", &[])
            .await
            .map_err(repo_error)?;
        u64::try_from(col::<i64>(&row, "n")?).map_err(corrupt)
    }
}

// ---------- settings ----------

impl<C: PgConnection> SearchSettingsRepo for PgStore<C> {
    async fn get(&self) -> Result<Option<SearchSettings>, RepoError> {
        let mut conn = self.conn().await;
        let row = conn
            .client()
            .query_typed_opt(
                &format!(
                    "SELECT version, split_by, facet_keys, facet_labels, {}
                     FROM search_settings",
                    stamp_columns("")
                ),
                &[],
            )
            .await
            .map_err(repo_error)?;
        let Some(row) = row else { return Ok(None) };
        let keys: Vec<String> = col(&row, "facet_keys")?;
        let labels: Vec<Option<String>> = col(&row, "facet_labels")?;
        SearchSettings::restore(SearchSettingsSnapshot {
            version: version_from_db(col(&row, "version")?)?,
            stamps: stamps_from_row(&row)?,
            split_by: col(&row, "split_by")?,
            facets: keys.into_iter().zip(labels).collect(),
        })
        .map(Some)
        .map_err(|e| corrupt(format!("search settings: {e}")))
    }

    async fn insert(&self, settings: &mut SearchSettings) -> Result<(), RepoError> {
        stamp_insert(settings, self.now());
        let s = settings.snapshot();
        let events = hypha_core::Recorded::take_events(settings);
        let actor = self.actor();
        let version = Version::NEW.next();
        let (keys, labels) = facet_columns(&s);
        in_transaction!(self, |tx| {
            tx.query_typed(
                "INSERT INTO search_settings (version, split_by, facet_keys, facet_labels)
                 VALUES (1, $1, $2, $3)",
                &[
                    (&s.split_by, Type::TEXT_ARRAY),
                    (&keys, Type::TEXT_ARRAY),
                    (&labels, Type::TEXT_ARRAY),
                ],
            )
            .await
            .map_err(repo_error)?;
            write_stamps(tx, "search_settings", "id", &true, Type::BOOL, s.stamps).await?;
            insert_events(tx, &events, Some(version), actor).await
        })?;
        settings.set_version(version);
        Ok(())
    }

    async fn update(&self, settings: &mut SearchSettings) -> Result<(), RepoError> {
        stamp_update(settings, self.now());
        let s = settings.snapshot();
        let events = hypha_core::Recorded::take_events(settings);
        let actor = self.actor();
        let (keys, labels) = facet_columns(&s);
        let version = in_transaction!(self, |tx| {
            let row = tx
                .query_typed_opt(
                    "UPDATE search_settings
                     SET version = version + 1, split_by = $2, facet_keys = $3,
                         facet_labels = $4
                     WHERE version = $1 RETURNING version",
                    &[
                        (&version_to_db(s.version)?, Type::INT8),
                        (&s.split_by, Type::TEXT_ARRAY),
                        (&keys, Type::TEXT_ARRAY),
                        (&labels, Type::TEXT_ARRAY),
                    ],
                )
                .await
                .map_err(repo_error)?
                .ok_or(RepoError::StaleVersion)?;
            let version = version_from_db(col(&row, "version")?)?;
            write_stamps(tx, "search_settings", "id", &true, Type::BOOL, s.stamps).await?;
            insert_events(tx, &events, Some(version), actor).await?;
            Ok(version)
        })?;
        settings.set_version(version);
        Ok(())
    }
}

fn facet_columns(s: &SearchSettingsSnapshot) -> (Vec<String>, Vec<Option<String>>) {
    s.facets.iter().cloned().unzip()
}
