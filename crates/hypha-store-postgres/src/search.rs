//! Search in Postgres: the built-in engine, the queue of what to rebuild, and the
//! merchant's settings.
//!
//! The engine answers exactly as `hypha_search_index::reference::search` (the
//! `search_index` contract checks it). Everything that defines a match or an order is
//! computed in Rust when a document is written: its terms (`SearchDocument::terms`;
//! `tsv` is `array_to_tsvector` of them, so Postgres's parser plays no part and the GIN
//! index alone decides a text match), its facet values, a lowercased title. SQL only
//! compares them, byte by byte (`COLLATE "C"`), so the database's locale changes
//! nothing.
//!
//! A search is one statement in one snapshot: the matching rows, narrow (no display
//! columns), materialised once; each group's representative (`DISTINCT ON`, skipped
//! for `group=variant`); the page; and from the same matched rows the total, the price
//! range and the facets without an active filter (one pass for all of them). A facet
//! that is filtered ignores its own filter, so it gets a pass of its own. Only the
//! page's rows are read whole. The default option facets come from
//! `search_option_counts`, kept as documents are written.

use std::collections::{BTreeMap, BTreeSet};

use hypha_core::{
    AssetId, CategoryId, ChannelId, MarketId, Money, ProductId, RepoError, Timestamp, VariantId,
    Version, Versioned, stamp_insert, stamp_update,
};
use hypha_search_index::{
    FacetCounts, FacetKey, FacetValue, Group, Hit, IndexError, MAX_FACET_VALUES, Mark, PriceRange,
    SearchDocument, SearchIndex, SearchQuery, SearchQueue, SearchResults, SearchSettings,
    SearchSettingsRepo, SearchSettingsSnapshot, Sort, Target, reference,
};
use tokio_postgres::{
    Row,
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
    store::Queries,
    store::{PgConnection, PgStore, consistent_read, in_transaction},
};

/// Separates a facet's key from its value in `facets` (`option.color` 0x1F `Red`).
const SEPARATOR: char = '\u{1f}';

/// Memory for a search's sorts and hashes, set for its transaction only: the default
/// (4 MB) spills a large market's grouping to disk.
const SEARCH_WORK_MEM: &str = "64MB";

/// Documents written per `INSERT`: Postgres takes at most 65,535 parameters.
const INSERT_ROWS: usize = 1000;

/// The columns a document is written with, in order (one placeholder each, but `tsv`).
const COLUMNS: &str =
    "market_id, product_id, variant_id, listing, channels, categories, handle, title, title_sort,
    variant_title, description, vendor, product_type, tags, sku, option_names, option_values,
    currency, price, product_min, product_max, listing_min, listing_max, available, image_id,
    created_us, position, terms_a, terms_b, terms_d, tsv, facets";
const PER_ROW: usize = 32;

// ---------- the engine ----------

impl<C: PgConnection> SearchIndex for PgStore<C> {
    async fn replace(
        &self,
        market: MarketId,
        products: &[ProductId],
        documents: &[SearchDocument],
    ) -> Result<(), IndexError> {
        if products.is_empty() {
            return Ok(());
        }
        let product_ids: Vec<Uuid> = products.iter().map(|p| p.as_uuid()).collect();
        // The option facets each document adds, to keep `search_option_counts` (the
        // market's) and `search_channel_option_counts` (each channel's in it).
        let mut counts: BTreeMap<String, i64> = BTreeMap::new();
        let mut channel_counts: BTreeMap<(Uuid, String), i64> = BTreeMap::new();
        for document in documents {
            for (key, _) in document.facet_values() {
                if key.starts_with("option.") {
                    for channel in &document.channels {
                        *channel_counts
                            .entry((channel.as_uuid(), key.clone()))
                            .or_default() += 1;
                    }
                    *counts.entry(key).or_default() += 1;
                }
            }
        }
        in_transaction!(self, |tx| {
            let gone = tx
                .query_typed(
                    "WITH gone AS (
                         DELETE FROM search_documents
                         WHERE market_id = $1 AND product_id = ANY($2)
                         RETURNING facets, channels
                     ),
                     options AS (
                         SELECT split_part(fv, chr(31), 1) AS key, gone.channels
                         FROM gone, unnest(gone.facets) fv WHERE starts_with(fv, 'option.')
                     )
                     SELECT NULL::uuid AS channel, key, count(*) AS n FROM options GROUP BY key
                     UNION ALL
                     SELECT c, key, count(*) FROM options, unnest(options.channels) c
                     GROUP BY c, key",
                    &[
                        (&market.as_uuid(), Type::UUID),
                        (&product_ids, Type::UUID_ARRAY),
                    ],
                )
                .await
                .map_err(repo_error)?;
            for row in &gone {
                let n = col::<i64>(row, "n")?;
                match col::<Option<Uuid>>(row, "channel")? {
                    None => *counts.entry(col(row, "key")?).or_default() -= n,
                    Some(channel) => {
                        *channel_counts
                            .entry((channel, col(row, "key")?))
                            .or_default() -= n
                    }
                }
            }
            for chunk in documents.chunks(INSERT_ROWS) {
                insert_documents(tx, chunk).await?;
            }
            // In key order, so runners updating the same market lock rows alike.
            let (keys, deltas): (Vec<String>, Vec<i64>) =
                counts.into_iter().filter(|(_, n)| *n != 0).unzip();
            if !keys.is_empty() {
                tx.query_typed(
                    "INSERT INTO search_option_counts (market_id, key, documents)
                     SELECT $1, k, n FROM unnest($2::text[], $3::bigint[]) AS c (k, n)
                     ORDER BY k COLLATE \"C\"
                     ON CONFLICT (market_id, key)
                     DO UPDATE SET documents = search_option_counts.documents + EXCLUDED.documents",
                    &[
                        (&market.as_uuid(), Type::UUID),
                        (&keys, Type::TEXT_ARRAY),
                        (&deltas, Type::INT8_ARRAY),
                    ],
                )
                .await
                .map_err(repo_error)?;
                tx.query_typed(
                    "DELETE FROM search_option_counts
                     WHERE market_id = $1 AND key = ANY($2) AND documents <= 0",
                    &[(&market.as_uuid(), Type::UUID), (&keys, Type::TEXT_ARRAY)],
                )
                .await
                .map_err(repo_error)?;
            }
            // The same per channel; the map's order is (channel, key), as the locks want.
            let mut channels = Vec::new();
            let mut keys = Vec::new();
            let mut deltas = Vec::new();
            for ((channel, key), n) in channel_counts.into_iter().filter(|(_, n)| *n != 0) {
                channels.push(channel);
                keys.push(key);
                deltas.push(n);
            }
            if !keys.is_empty() {
                tx.query_typed(
                    "INSERT INTO search_channel_option_counts (market_id, channel_id, key, documents)
                     SELECT $1, c, k, n FROM unnest($2::uuid[], $3::text[], $4::bigint[])
                         AS t (c, k, n)
                     ORDER BY c, k COLLATE \"C\"
                     ON CONFLICT (market_id, channel_id, key)
                     DO UPDATE SET documents =
                         search_channel_option_counts.documents + EXCLUDED.documents",
                    &[
                        (&market.as_uuid(), Type::UUID),
                        (&channels, Type::UUID_ARRAY),
                        (&keys, Type::TEXT_ARRAY),
                        (&deltas, Type::INT8_ARRAY),
                    ],
                )
                .await
                .map_err(repo_error)?;
                tx.query_typed(
                    "DELETE FROM search_channel_option_counts
                     WHERE market_id = $1 AND documents <= 0
                       AND (channel_id, key) IN (SELECT * FROM unnest($2::uuid[], $3::text[]))",
                    &[
                        (&market.as_uuid(), Type::UUID),
                        (&channels, Type::UUID_ARRAY),
                        (&keys, Type::TEXT_ARRAY),
                    ],
                )
                .await
                .map_err(repo_error)?;
            }
            Ok(())
        })?;
        Ok(())
    }

    async fn search(&self, query: &SearchQuery) -> Result<SearchResults, IndexError> {
        let mut conn = self.conn().await;
        let results = consistent_read!(conn, |tx| {
            tx.batch_execute(&format!("SET LOCAL work_mem = '{SEARCH_WORK_MEM}'"))
                .await
                .map_err(repo_error)?;
            run_search(tx, query).await
        })?;
        Ok(results)
    }

    async fn option_names(
        &self,
        market: MarketId,
        channel: Option<ChannelId>,
    ) -> Result<Vec<String>, IndexError> {
        let mut conn = self.conn().await;
        let rows = match channel {
            None => {
                conn.db()
                    .query_typed(
                        "SELECT key FROM search_option_counts WHERE market_id = $1
                         ORDER BY documents DESC, key COLLATE \"C\"",
                        &[(&market.as_uuid(), Type::UUID)],
                    )
                    .await
            }
            Some(channel) => {
                conn.db()
                    .query_typed(
                        "SELECT key FROM search_channel_option_counts
                         WHERE market_id = $1 AND channel_id = $2
                         ORDER BY documents DESC, key COLLATE \"C\"",
                        &[
                            (&market.as_uuid(), Type::UUID),
                            (&channel.as_uuid(), Type::UUID),
                        ],
                    )
                    .await
            }
        }
        .map_err(repo_error)?;
        Ok(rows
            .iter()
            .map(|row| {
                let key: String = col(row, "key")?;
                match key.parse::<FacetKey>() {
                    Ok(FacetKey::Option(name)) => Ok(name),
                    _ => Err(corrupt(format!("search option key {key:?}"))),
                }
            })
            .collect::<Result<Vec<_>, RepoError>>()?)
    }

    async fn count(&self) -> Result<u64, IndexError> {
        let mut conn = self.conn().await;
        let row = conn
            .db()
            .query_typed_one("SELECT count(*) AS n FROM search_documents", &[])
            .await
            .map_err(repo_error)?;
        let n: i64 = col(&row, "n")?;
        Ok(u64::try_from(n).map_err(corrupt)?)
    }
}

/// One `INSERT` for `documents` (at most `INSERT_ROWS`).
async fn insert_documents(
    tx: &impl Queries,
    documents: &[SearchDocument],
) -> Result<(), RepoError> {
    if documents.is_empty() {
        return Ok(());
    }
    let mut params = Params::default();
    let mut rows = Vec::with_capacity(documents.len());
    for d in documents {
        let terms = d.terms();
        let all_terms = terms.all();
        let (names, values): (Vec<String>, Vec<String>) = d.options.iter().cloned().unzip();
        let facets: Vec<String> = d
            .facet_values()
            .into_iter()
            .map(|(key, value)| format!("{key}{SEPARATOR}{value}"))
            .collect();
        let position = i32::try_from(d.position).map_err(corrupt)?;
        let placeholders = [
            params.add(d.market.as_uuid(), Type::UUID),
            params.add(d.product.as_uuid(), Type::UUID),
            params.add(d.variant.as_uuid(), Type::UUID),
            params.add(d.listing.clone(), Type::TEXT),
            params.add(
                d.channels
                    .iter()
                    .map(|c| c.as_uuid())
                    .collect::<Vec<Uuid>>(),
                Type::UUID_ARRAY,
            ),
            params.add(
                d.categories
                    .iter()
                    .map(|c| c.as_uuid())
                    .collect::<Vec<Uuid>>(),
                Type::UUID_ARRAY,
            ),
            params.add(d.handle.clone(), Type::TEXT),
            params.add(d.title.clone(), Type::TEXT),
            params.add(d.title.to_lowercase(), Type::TEXT),
            params.add(d.variant_title.clone(), Type::TEXT),
            params.add(d.description.clone(), Type::TEXT),
            params.add(d.vendor.clone(), Type::TEXT),
            params.add(d.product_type.clone(), Type::TEXT),
            params.add(d.tags.clone(), Type::TEXT_ARRAY),
            params.add(d.sku.clone(), Type::TEXT),
            params.add(names, Type::TEXT_ARRAY),
            params.add(values, Type::TEXT_ARRAY),
            params.add(d.price.currency().as_str().to_string(), Type::TEXT),
            params.add(d.price.amount(), Type::INT8),
            params.add(d.product_price.min.amount(), Type::INT8),
            params.add(d.product_price.max.amount(), Type::INT8),
            params.add(d.listing_price.min.amount(), Type::INT8),
            params.add(d.listing_price.max.amount(), Type::INT8),
            params.add(d.available, Type::BOOL),
            params.add(d.image.map(|a| a.as_uuid()), Type::UUID),
            params.add(d.created_at.unix_micros(), Type::INT8),
            params.add(position, Type::INT4),
            params.add(terms.a, Type::TEXT_ARRAY),
            params.add(terms.b, Type::TEXT_ARRAY),
            params.add(terms.d, Type::TEXT_ARRAY),
            format!(
                "array_to_tsvector({}::text[])",
                params.add(all_terms, Type::TEXT_ARRAY)
            ),
            params.add(facets, Type::TEXT_ARRAY),
        ];
        debug_assert_eq!(placeholders.len(), PER_ROW);
        rows.push(format!("({})", placeholders.join(", ")));
    }
    tx.query_typed(
        &format!(
            "INSERT INTO search_documents ({COLUMNS}) VALUES {}",
            rows.join(", ")
        ),
        &params.as_slice(),
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
        // Every term a prefix of one of the document's terms: exactly `tsv @@` with
        // prefix lexemes, since `tsv` holds the same terms. Terms are letters and digits
        // (`terms::terms`), so quoting them needs no escapes.
        let tsquery: Vec<String> = q.terms.iter().map(|t| format!("'{t}':*")).collect();
        sql.push(format!(
            "d.tsv @@ {}::tsquery",
            params.add(tsquery.join(" & "), Type::TEXT)
        ));
    }
    if let Some(channel) = q.channel {
        // GIN-indexed: `@>` (contains) is what the index answers, not `= ANY`.
        sql.push(format!(
            "d.channels @> ARRAY[{}]::uuid[]",
            params.add(channel.as_uuid(), Type::UUID)
        ));
    }
    if let Some(category) = q.category {
        // GIN-indexed like channels: a document carries its categories' ancestors too.
        sql.push(format!(
            "d.categories @> ARRAY[{}]::uuid[]",
            params.add(category.as_uuid(), Type::UUID)
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

/// The group key of `d`, as compared and hashed (bytes, never the locale's order).
fn group_expr(group: Group) -> &'static str {
    match group {
        Group::Listing => "d.listing COLLATE \"C\"",
        Group::Product => "d.product_id::text",
        Group::Variant => "d.variant_id::text",
    }
}

/// `reference::score` in SQL: per query term, 10, 4 or 1 for the best field it
/// prefixes a term of, summed. Only computed when the sort needs it.
fn score_expr(q: &SearchQuery, sort: Sort, params: &mut Params) -> String {
    if q.terms.is_empty() || sort != Sort::Relevance {
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

/// `keys`' facet values with how many groups have them, as `(fv, n)` rows, from
/// `source`'s rows (each with its `facets` and its group `g`). `distinct`: a group can
/// have several of the rows (else each row is its own group, and counting is enough).
fn facet_branch(source: &str, keys: &[&FacetKey], distinct: bool, params: &mut Params) -> String {
    let prefixes: Vec<String> = keys.iter().map(|k| format!("{k}{SEPARATOR}")).collect();
    let prefixes = params.add(prefixes, Type::TEXT_ARRAY);
    let of_keys = format!("split_part(fv, chr(31), 1) || chr(31) = ANY({prefixes}::text[])");
    if distinct {
        format!(
            "SELECT fv, count(*) AS n FROM (
                 SELECT DISTINCT fv, g FROM {source}, unnest(facets) fv WHERE {of_keys}
             ) x GROUP BY fv"
        )
    } else {
        format!(
            "SELECT fv, count(*) AS n FROM {source}, unnest(facets) fv WHERE {of_keys}
             GROUP BY fv"
        )
    }
}

/// Vendor, product type and tags are the product's, the same on all its variants: a listing
/// or product group has one set of them, so they count once per group, from its
/// representative, instead of over every matching variant. Custom fields aren't: a
/// variant's value wins over its product's.
fn product_level(key: &FacetKey) -> bool {
    !matches!(key, FacetKey::Option(_) | FacetKey::Field(_))
}

async fn run_search(tx: &impl Queries, q: &SearchQuery) -> Result<SearchResults, RepoError> {
    let sort = reference::effective_sort(q);
    let group = group_expr(q.group);
    let mut params = Params::default();
    let matched = conditions(q, &mut params, Except::Nothing);
    let score = score_expr(q, sort, &mut params);
    let market = params.add(q.market.as_uuid(), Type::UUID);

    // Each group's representative, then the page: the reference's orders.
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
    let between = format!(
        "{by_sort}created_us DESC, product_id DESC, listing COLLATE \"C\" ASC, position ASC"
    );
    // Narrow: the facets stay behind, so the sort that collapses groups stays small.
    let best_columns =
        "variant_id, product_id, g, score, price, created_us, title_sort, listing, position";
    let best = match q.group {
        // Every variant is its own group: nothing to collapse.
        Group::Variant => format!("SELECT {best_columns} FROM m"),
        _ => format!("SELECT DISTINCT ON (g) {best_columns} FROM m ORDER BY g, {within}"),
    };
    let offset = params.add(i64::from(q.offset), Type::INT8);
    let limit = params.add(i64::from(q.limit), Type::INT8);

    // The price range ignores the price bounds: from the matched rows when there are
    // none, else from a pass of its own.
    let price = if q.price.min.is_none() && q.price.max.is_none() {
        "(SELECT min(price) FROM m) AS low, (SELECT max(price) FROM m) AS high".to_string()
    } else {
        let unbounded = conditions(q, &mut params, Except::Price);
        format!(
            "(SELECT min(d.price) FROM search_documents d WHERE {unbounded}) AS low,
             (SELECT max(d.price) FROM search_documents d WHERE {unbounded}) AS high"
        )
    };

    // Facets: the unfiltered ones from the matched rows (product-level ones from the
    // groups' representatives); each filtered one over the rows that pass every other
    // condition.
    let filtered: BTreeSet<&FacetKey> = q.filters.iter().map(|f| &f.key).collect();
    let free: Vec<&FacetKey> = q.facets.iter().filter(|k| !filtered.contains(k)).collect();
    let grouped = q.group != Group::Variant;
    let (per_product, per_variant): (Vec<&FacetKey>, Vec<&FacetKey>) = if grouped {
        free.iter().partition(|k| product_level(k))
    } else {
        (vec![], free.clone())
    };
    let mut branches = Vec::new();
    if !per_product.is_empty() {
        let representatives = format!(
            "(SELECT b.g, d.facets FROM best b JOIN search_documents d
              ON d.market_id = {market} AND d.variant_id = b.variant_id) r"
        );
        branches.push(facet_branch(
            &representatives,
            &per_product,
            false,
            &mut params,
        ));
    }
    if !per_variant.is_empty() {
        branches.push(facet_branch("m", &per_variant, grouped, &mut params));
    }
    for key in q.facets.iter().filter(|k| filtered.contains(k)) {
        let conds = conditions(q, &mut params, Except::Facet(key));
        let source =
            format!("(SELECT {group} AS g, d.facets FROM search_documents d WHERE {conds}) own");
        branches.push(facet_branch(&source, &[key], grouped, &mut params));
    }
    let facets = if branches.is_empty() {
        "NULL::text[] AS facet_values, NULL::bigint[] AS facet_counts".to_string()
    } else {
        "(SELECT array_agg(fv) FROM f) AS facet_values,
         (SELECT array_agg(n) FROM f) AS facet_counts"
            .to_string()
    };
    let facet_cte = if branches.is_empty() {
        String::new()
    } else {
        format!(", f AS MATERIALIZED ({})", branches.join(" UNION ALL "))
    };

    let sql = format!(
        "WITH m AS MATERIALIZED (
             SELECT d.variant_id, d.product_id, {group} AS g, {score} AS score, d.price,
                    d.created_us, d.title_sort, d.listing, d.position, d.facets
             FROM search_documents d WHERE {matched}
         ),
         best AS MATERIALIZED ({best}),
         page AS (SELECT * FROM best ORDER BY {between} OFFSET {offset} LIMIT {limit}),
         counts AS (
             SELECT g, count(*) AS matched FROM m
             WHERE g IN (SELECT g FROM page) GROUP BY g
         ){facet_cte},
         summary AS (
             SELECT (SELECT count(*) FROM best) AS total, {price}, {facets},
                    (SELECT currency FROM search_documents WHERE market_id = {market} LIMIT 1)
                        AS market_currency
         )
         SELECT s.total, s.low, s.high, s.facet_values, s.facet_counts, s.market_currency,
                p.variant_id AS hit, c.matched, d.*
         FROM summary s
         LEFT JOIN page p ON true
         LEFT JOIN counts c ON c.g = p.g
         LEFT JOIN search_documents d ON d.market_id = {market} AND d.variant_id = p.variant_id
         ORDER BY {}",
        between
            .split(", ")
            .map(|key| format!("p.{key}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let rows = tx
        .query_typed(&sql, &params.as_slice())
        .await
        .map_err(repo_error)?;
    let first = rows
        .first()
        .ok_or_else(|| corrupt("a search returned no summary"))?;

    let total = u64::try_from(col::<i64>(first, "total")?).map_err(corrupt)?;
    let price = match (
        col::<Option<i64>>(first, "low")?,
        col::<Option<i64>>(first, "high")?,
        col::<Option<String>>(first, "market_currency")?,
    ) {
        (Some(low), Some(high), Some(currency)) => {
            let currency = currency_from_db(&currency)?;
            let money = |n| Money::new(n, currency).map_err(corrupt);
            Some((money(low)?, money(high)?))
        }
        _ => None,
    };
    let hits = rows
        .iter()
        .filter(|row| matches!(col::<Option<Uuid>>(row, "hit"), Ok(Some(_))))
        .map(|row| {
            Ok(Hit {
                document: document_from_row(row)?,
                matched: u32::try_from(col::<i64>(row, "matched")?).map_err(corrupt)?,
            })
        })
        .collect::<Result<Vec<_>, RepoError>>()?;

    let values: Vec<String> =
        col::<Option<Vec<String>>>(first, "facet_values")?.unwrap_or_default();
    let counts: Vec<i64> = col::<Option<Vec<i64>>>(first, "facet_counts")?.unwrap_or_default();
    let mut by_key: BTreeMap<String, Vec<FacetValue>> = BTreeMap::new();
    for (entry, n) in values.into_iter().zip(counts) {
        let (key, value) = entry
            .split_once(SEPARATOR)
            .ok_or_else(|| corrupt(format!("search facet {entry:?}")))?;
        by_key.entry(key.to_string()).or_default().push(FacetValue {
            value: value.to_string(),
            count: u64::try_from(n).map_err(corrupt)?,
        });
    }
    let facets = q
        .facets
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
        .collect();

    Ok(SearchResults {
        hits,
        total,
        facets,
        price,
    })
}

fn document_from_row(row: &Row) -> Result<SearchDocument, RepoError> {
    let currency = currency_from_db(&col::<String>(row, "currency")?)?;
    let money = |column: &str| -> Result<Money, RepoError> {
        Money::new(col(row, column)?, currency).map_err(corrupt)
    };
    let names: Vec<String> = col(row, "option_names")?;
    let values: Vec<String> = col(row, "option_values")?;
    // Custom field values live only in `facets`, in the document's order: no column of
    // their own.
    let field_prefix = FacetKey::Field(String::new()).to_string();
    let fields = col::<Vec<String>>(row, "facets")?
        .into_iter()
        .filter_map(|entry| {
            let (key, value) = entry.split_once(SEPARATOR)?;
            let key = key.strip_prefix(&field_prefix)?;
            Some((key.to_string(), value.to_string()))
        })
        .collect();
    Ok(SearchDocument {
        market: MarketId::from_uuid(col(row, "market_id")?),
        product: ProductId::from_uuid(col(row, "product_id")?),
        variant: VariantId::from_uuid(col(row, "variant_id")?),
        listing: col(row, "listing")?,
        channels: col::<Vec<Uuid>>(row, "channels")?
            .into_iter()
            .map(ChannelId::from_uuid)
            .collect(),
        categories: col::<Vec<Uuid>>(row, "categories")?
            .into_iter()
            .map(CategoryId::from_uuid)
            .collect(),
        handle: col(row, "handle")?,
        title: col(row, "title")?,
        variant_title: col(row, "variant_title")?,
        description: col(row, "description")?,
        vendor: col(row, "vendor")?,
        product_type: col(row, "product_type")?,
        tags: col(row, "tags")?,
        sku: col(row, "sku")?,
        options: names.into_iter().zip(values).collect(),
        fields,
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
        Target::Category(id) => format!("category:{}", id.as_uuid()),
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
        Some(("category", id)) => Ok(Target::Category(CategoryId::from_uuid(
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
        conn.db()
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
            .db()
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

    async fn finish(&self, marks: &[Mark]) -> Result<Vec<Target>, RepoError> {
        if marks.is_empty() {
            return Ok(vec![]);
        }
        let targets: Vec<String> = marks.iter().map(|m| target_to_db(m.target)).collect();
        let generations: Vec<i64> = marks.iter().map(|m| m.generation).collect();
        let (finished, present) = in_transaction!(self, |tx| {
            let targets_of = |rows: Vec<Row>| {
                rows.iter()
                    .map(|row| col::<String>(row, "target"))
                    .collect::<Result<BTreeSet<String>, RepoError>>()
            };
            let deleted = tx
                .query_typed(
                    "DELETE FROM search_queue q
                     USING unnest($1::text[], $2::bigint[]) AS f (target, generation)
                     WHERE q.target = f.target AND q.generation = f.generation
                     RETURNING q.target",
                    &[
                        (&targets, Type::TEXT_ARRAY),
                        (&generations, Type::INT8_ARRAY),
                    ],
                )
                .await
                .map_err(repo_error)?;
            // Marked again meanwhile: free for the next run at once.
            let kept = tx
                .query_typed(
                    "UPDATE search_queue SET leased_until = NULL WHERE target = ANY($1)
                     RETURNING target",
                    &[(&targets, Type::TEXT_ARRAY)],
                )
                .await
                .map_err(repo_error)?;
            Ok((targets_of(deleted)?, targets_of(kept)?))
        })?;
        // Neither finished now nor still queued: someone else finished it.
        Ok(marks
            .iter()
            .zip(&targets)
            .filter(|(_, t)| !finished.contains(*t) && !present.contains(*t))
            .map(|(m, _)| m.target)
            .collect())
    }

    async fn queued(&self) -> Result<u64, RepoError> {
        let mut conn = self.conn().await;
        let row = conn
            .db()
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
            .db()
            .query_typed_opt(
                &format!(
                    "SELECT version, split_by, facet_keys, facet_labels, search_app, {}
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
            search_app: col(&row, "search_app")?,
        })
        .map(Some)
        .map_err(|e| corrupt(format!("search settings: {e}")))
    }

    async fn insert(&self, settings: &mut SearchSettings) -> Result<(), RepoError> {
        stamp_insert(settings, self.now());
        let s = settings.snapshot();
        let events = hypha_core::Recorded::take_events(settings);
        let stamp = self.event_stamp();
        let version = Version::NEW.next();
        let (keys, labels) = facet_columns(&s);
        in_transaction!(self, |tx| {
            tx.query_typed(
                "INSERT INTO search_settings
                     (version, split_by, facet_keys, facet_labels, search_app)
                 VALUES (1, $1, $2, $3, $4)",
                &[
                    (&s.split_by, Type::TEXT_ARRAY),
                    (&keys, Type::TEXT_ARRAY),
                    (&labels, Type::TEXT_ARRAY),
                    (&s.search_app, Type::TEXT),
                ],
            )
            .await
            .map_err(repo_error)?;
            write_stamps(tx, "search_settings", "id", &true, Type::BOOL, s.stamps).await?;
            insert_events(tx, &events, Some(version), stamp).await
        })?;
        settings.set_version(version);
        Ok(())
    }

    async fn update(&self, settings: &mut SearchSettings) -> Result<(), RepoError> {
        stamp_update(settings, self.now());
        let s = settings.snapshot();
        let events = hypha_core::Recorded::take_events(settings);
        let stamp = self.event_stamp();
        let (keys, labels) = facet_columns(&s);
        let version = in_transaction!(self, |tx| {
            let row = tx
                .query_typed_opt(
                    "UPDATE search_settings
                     SET version = version + 1, split_by = $2, facet_keys = $3,
                         facet_labels = $4, search_app = $5
                     WHERE version = $1 RETURNING version",
                    &[
                        (&version_to_db(s.version)?, Type::INT8),
                        (&s.split_by, Type::TEXT_ARRAY),
                        (&keys, Type::TEXT_ARRAY),
                        (&labels, Type::TEXT_ARRAY),
                        (&s.search_app, Type::TEXT),
                    ],
                )
                .await
                .map_err(repo_error)?
                .ok_or(RepoError::StaleVersion)?;
            let version = version_from_db(col(&row, "version")?)?;
            write_stamps(tx, "search_settings", "id", &true, Type::BOOL, s.stamps).await?;
            insert_events(tx, &events, Some(version), stamp).await?;
            Ok(version)
        })?;
        settings.set_version(version);
        Ok(())
    }
}

fn facet_columns(s: &SearchSettingsSnapshot) -> (Vec<String>, Vec<Option<String>>) {
    s.facets.iter().cloned().unzip()
}
