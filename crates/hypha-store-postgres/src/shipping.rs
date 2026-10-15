//! Shipping zones (with options and rate tiers) and app providers (design note 0015).
//! A zone and its children load in four queries and are written wholesale, like
//! products.

use std::collections::HashMap;

use hypha_core::{
    CountryCode, Currency, Money, Price, Recorded, RepoError, Sealed, ShippingOptionId,
    ShippingZoneId, TaxClassId, Version, Versioned,
};
use hypha_shipping::{
    AppProvider, AppProviderRepo, AppProviderSnapshot, BuiltinRate, DeliveryEstimate, NewOption,
    OptionRate, ProviderKey, RateBasis, RateTable, ServiceInfo, ShippingZone, ShippingZoneRepo,
    ShippingZoneSnapshot, Tier,
};
use tokio_postgres::{GenericClient, types::Type};
use uuid::Uuid;

use crate::{
    columns::{col, currency_from_db, version_from_db, version_to_db},
    error::{corrupt, repo_error},
    outbox::insert_events,
    store::{PgConnection, PgStore, consistent_read, in_transaction},
};

// ---------- zones ----------

impl<C: PgConnection> ShippingZoneRepo for PgStore<C> {
    async fn get(&self, id: ShippingZoneId) -> Result<Option<ShippingZone>, RepoError> {
        let mut conn = self.conn().await;
        consistent_read!(conn.client(), |tx| {
            Ok(load_zones(tx, Some(&[id.as_uuid()])).await?.pop())
        })
    }

    async fn get_by_country(
        &self,
        country: CountryCode,
    ) -> Result<Option<ShippingZone>, RepoError> {
        let mut conn = self.conn().await;
        consistent_read!(conn.client(), |tx| {
            let row = tx
                .query_typed_opt(
                    "SELECT zone_id FROM shipping_zone_countries WHERE country = $1",
                    &[(&country.as_str(), Type::TEXT)],
                )
                .await
                .map_err(repo_error)?;
            match row {
                Some(row) => Ok(load_zones(tx, Some(&[col(&row, "zone_id")?])).await?.pop()),
                None => Ok(None),
            }
        })
    }

    async fn list(&self) -> Result<Vec<ShippingZone>, RepoError> {
        let mut conn = self.conn().await;
        consistent_read!(conn.client(), |tx| load_zones(tx, None).await)
    }

    async fn insert(&self, zone: &mut ShippingZone) -> Result<(), RepoError> {
        let s = zone.snapshot();
        let events = zone.take_events();
        let actor = self.actor();
        let version = Version::NEW.next();
        in_transaction!(self, |tx| {
            insert_zone(tx, &s).await?;
            insert_events(tx, &events, Some(version), actor).await
        })?;
        zone.set_version(version);
        Ok(())
    }

    async fn update(&self, zone: &mut ShippingZone) -> Result<(), RepoError> {
        let s = zone.snapshot();
        let events = zone.take_events();
        let actor = self.actor();
        let version = in_transaction!(self, |tx| {
            let version = update_zone(tx, &s).await?;
            insert_events(tx, &events, Some(version), actor).await?;
            Ok(version)
        })?;
        zone.set_version(version);
        Ok(())
    }

    async fn delete(&self, zone: &mut ShippingZone) -> Result<(), RepoError> {
        let id = zone.id();
        let events = zone.take_events();
        let actor = self.actor();
        in_transaction!(self, |tx| {
            delete_zone(tx, id).await?;
            // No version: the zone is gone.
            insert_events(tx, &events, None, actor).await
        })
    }
}

/// `ShippingZoneRepo::insert` inside the caller's transaction: the row, then its
/// countries, options and tiers.
pub(crate) async fn insert_zone(
    tx: &impl GenericClient,
    s: &ShippingZoneSnapshot,
) -> Result<(), RepoError> {
    tx.query_typed(
        "INSERT INTO shipping_zones (id, version, name) VALUES ($1, 1, $2)",
        &[(&s.id.as_uuid(), Type::UUID), (&s.name, Type::TEXT)],
    )
    .await
    .map_err(repo_error)?;
    write_zone_children(tx, s).await
}

/// `ShippingZoneRepo::update` inside the caller's transaction; the new version. The
/// version check comes first; nothing else is touched if it fails.
pub(crate) async fn update_zone(
    tx: &impl GenericClient,
    s: &ShippingZoneSnapshot,
) -> Result<Version, RepoError> {
    let bumped = tx
        .query_typed_opt(
            "UPDATE shipping_zones SET version = version + 1, name = $3
             WHERE id = $1 AND version = $2 RETURNING version",
            &[
                (&s.id.as_uuid(), Type::UUID),
                (&version_to_db(s.version)?, Type::INT8),
                (&s.name, Type::TEXT),
            ],
        )
        .await
        .map_err(repo_error)?;
    let row = bumped.ok_or(RepoError::StaleVersion)?;
    // Children are replaced wholesale; tiers go with their options (CASCADE).
    for sql in [
        "DELETE FROM shipping_zone_countries WHERE zone_id = $1",
        "DELETE FROM shipping_options WHERE zone_id = $1",
    ] {
        tx.query_typed(sql, &[(&s.id.as_uuid(), Type::UUID)])
            .await
            .map_err(repo_error)?;
    }
    write_zone_children(tx, s).await?;
    version_from_db(col(&row, "version")?)
}

/// `ShippingZoneRepo::delete` inside the caller's transaction; its children go with it
/// (CASCADE).
pub(crate) async fn delete_zone(
    tx: &impl GenericClient,
    id: ShippingZoneId,
) -> Result<(), RepoError> {
    tx.query_typed(
        "DELETE FROM shipping_zones WHERE id = $1",
        &[(&id.as_uuid(), Type::UUID)],
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}

/// Countries, options and tiers, a statement each however many there are.
async fn write_zone_children(
    tx: &impl GenericClient,
    s: &ShippingZoneSnapshot,
) -> Result<(), RepoError> {
    let zone = s.id.as_uuid();
    let countries: Vec<&str> = s.countries.iter().map(|c| c.as_str()).collect();
    tx.query_typed(
        "INSERT INTO shipping_zone_countries (country, zone_id, position)
         SELECT c.country, $1, c.ord - 1 FROM unnest($2::text[]) WITH ORDINALITY AS c(country, ord)",
        &[(&zone, Type::UUID), (&countries, Type::TEXT_ARRAY)],
    )
    .await
    .map_err(repo_error)?;

    let mut ids = Vec::new();
    let mut names = Vec::new();
    let mut codes = Vec::new();
    let mut bases: Vec<Option<&str>> = Vec::new();
    let mut provider_keys: Vec<Option<&str>> = Vec::new();
    let mut services: Vec<Option<&str>> = Vec::new();
    let mut min_days: Vec<Option<i32>> = Vec::new();
    let mut max_days: Vec<Option<i32>> = Vec::new();
    let mut tax_classes: Vec<Option<Uuid>> = Vec::new();
    let mut backups = Vec::new();
    let mut enabled = Vec::new();
    // Tiers: option, table position, position, currency, bound (as text), amount.
    let mut tier_options = Vec::new();
    let mut tier_tables = Vec::new();
    let mut tier_positions = Vec::new();
    let mut tier_currencies = Vec::new();
    let mut tier_bounds: Vec<Option<String>> = Vec::new();
    let mut tier_amounts = Vec::new();
    for o in &s.options {
        ids.push(o.id.as_uuid());
        names.push(o.name.as_str());
        codes.push(o.code.as_str());
        match &o.rate {
            OptionRate::Builtin(rate) => {
                bases.push(Some(rate.basis().as_str()));
                provider_keys.push(None);
                services.push(None);
                for (t, table) in (0i32..).zip(rate.tables()) {
                    for (p, tier) in (0i32..).zip(table.tiers()) {
                        tier_options.push(o.id.as_uuid());
                        tier_tables.push(t);
                        tier_positions.push(p);
                        tier_currencies.push(table.currency().as_str());
                        tier_bounds.push(tier.up_to.map(|b| b.to_string()));
                        tier_amounts.push(tier.price.money().amount());
                    }
                }
            }
            OptionRate::Provider { provider, service } => {
                bases.push(None);
                provider_keys.push(Some(provider.as_str()));
                services.push(Some(service.as_str()));
            }
        }
        min_days.push(o.estimate.map(|e| i32::from(e.min_days())));
        max_days.push(o.estimate.map(|e| i32::from(e.max_days())));
        tax_classes.push(o.tax_class.map(|c| c.as_uuid()));
        backups.push(o.is_backup);
        enabled.push(o.enabled);
    }
    tx.query_typed(
        "INSERT INTO shipping_options
            (id, zone_id, position, name, code, rate_basis, provider_key, provider_service,
             estimate_min_days, estimate_max_days, tax_class_id, is_backup, enabled)
         SELECT o.id, $1, o.ord - 1, o.name, o.code, o.basis, o.provider, o.service,
                o.min_days, o.max_days, o.tax_class, o.backup, o.enabled
         FROM unnest($2::uuid[], $3::text[], $4::text[], $5::text[], $6::text[], $7::text[],
                     $8::int4[], $9::int4[], $10::uuid[], $11::bool[], $12::bool[])
              WITH ORDINALITY AS o(id, name, code, basis, provider, service, min_days,
                                   max_days, tax_class, backup, enabled, ord)",
        &[
            (&zone, Type::UUID),
            (&ids, Type::UUID_ARRAY),
            (&names, Type::TEXT_ARRAY),
            (&codes, Type::TEXT_ARRAY),
            (&bases, Type::TEXT_ARRAY),
            (&provider_keys, Type::TEXT_ARRAY),
            (&services, Type::TEXT_ARRAY),
            (&min_days, Type::INT4_ARRAY),
            (&max_days, Type::INT4_ARRAY),
            (&tax_classes, Type::UUID_ARRAY),
            (&backups, Type::BOOL_ARRAY),
            (&enabled, Type::BOOL_ARRAY),
        ],
    )
    .await
    .map_err(repo_error)?;

    tx.query_typed(
        "INSERT INTO shipping_option_tiers
            (option_id, table_position, position, currency, up_to, amount)
         SELECT t.option_id, t.table_position, t.position, t.currency, t.up_to::numeric, t.amount
         FROM unnest($1::uuid[], $2::int4[], $3::int4[], $4::text[], $5::text[], $6::int8[])
              AS t(option_id, table_position, position, currency, up_to, amount)",
        &[
            (&tier_options, Type::UUID_ARRAY),
            (&tier_tables, Type::INT4_ARRAY),
            (&tier_positions, Type::INT4_ARRAY),
            (&tier_currencies, Type::TEXT_ARRAY),
            (&tier_bounds, Type::TEXT_ARRAY),
            (&tier_amounts, Type::INT8_ARRAY),
        ],
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}

/// The zones with these IDs (or all of them), by name then ID, in four queries.
async fn load_zones(
    client: &impl GenericClient,
    ids: Option<&[Uuid]>,
) -> Result<Vec<ShippingZone>, RepoError> {
    let ids: Option<Vec<Uuid>> = ids.map(<[Uuid]>::to_vec);
    let zones = client
        .query_typed(
            "SELECT id, version, name FROM shipping_zones
             WHERE $1::uuid[] IS NULL OR id = ANY($1) ORDER BY name, id",
            &[(&ids, Type::UUID_ARRAY)],
        )
        .await
        .map_err(repo_error)?;
    if zones.is_empty() {
        return Ok(vec![]);
    }
    let found: Vec<Uuid> = zones
        .iter()
        .map(|r| col(r, "id"))
        .collect::<Result<_, RepoError>>()?;
    let param = [(
        &found as &(dyn tokio_postgres::types::ToSql + Sync),
        Type::UUID_ARRAY,
    )];

    let mut countries: HashMap<Uuid, Vec<CountryCode>> = HashMap::new();
    for row in client
        .query_typed(
            "SELECT zone_id, country FROM shipping_zone_countries
             WHERE zone_id = ANY($1) ORDER BY zone_id, position",
            &param,
        )
        .await
        .map_err(repo_error)?
    {
        let code: String = col(&row, "country")?;
        let country = CountryCode::new(&code).map_err(corrupt)?;
        countries
            .entry(col(&row, "zone_id")?)
            .or_default()
            .push(country);
    }

    // Tiers by option, then by table, in order.
    let mut tiers: HashMap<Uuid, Vec<(i32, Currency, Tier)>> = HashMap::new();
    for row in client
        .query_typed(
            "SELECT t.option_id, t.table_position, t.currency, t.up_to::text, t.amount
             FROM shipping_option_tiers t JOIN shipping_options o ON o.id = t.option_id
             WHERE o.zone_id = ANY($1)
             ORDER BY t.option_id, t.table_position, t.position",
            &param,
        )
        .await
        .map_err(repo_error)?
    {
        let currency = currency_from_db(col(&row, "currency")?)?;
        let up_to = col::<Option<String>>(&row, "up_to")?
            .map(|b| b.parse::<u64>())
            .transpose()
            .map_err(corrupt)?;
        let price = Money::new(col(&row, "amount")?, currency)
            .and_then(Price::new)
            .map_err(corrupt)?;
        tiers.entry(col(&row, "option_id")?).or_default().push((
            col(&row, "table_position")?,
            currency,
            Tier { up_to, price },
        ));
    }

    let mut options: HashMap<Uuid, Vec<NewOption>> = HashMap::new();
    for row in client
        .query_typed(
            "SELECT zone_id, id, name, code, rate_basis, provider_key, provider_service,
                    estimate_min_days, estimate_max_days, tax_class_id, is_backup, enabled
             FROM shipping_options WHERE zone_id = ANY($1) ORDER BY zone_id, position",
            &param,
        )
        .await
        .map_err(repo_error)?
    {
        let id: Uuid = col(&row, "id")?;
        let rate = match col::<Option<String>>(&row, "rate_basis")? {
            Some(basis) => {
                let basis = match basis.as_str() {
                    "flat" => RateBasis::Flat,
                    "subtotal" => RateBasis::Subtotal,
                    "weight" => RateBasis::Weight,
                    other => return Err(corrupt(format!("rate basis {other:?}"))),
                };
                OptionRate::Builtin(builtin_rate(basis, tiers.remove(&id).unwrap_or_default())?)
            }
            None => OptionRate::Provider {
                provider: ProviderKey::new(&col::<String>(&row, "provider_key")?)
                    .map_err(corrupt)?,
                service: col(&row, "provider_service")?,
            },
        };
        let estimate = match (
            col::<Option<i32>>(&row, "estimate_min_days")?,
            col::<Option<i32>>(&row, "estimate_max_days")?,
        ) {
            (Some(min), Some(max)) => Some(
                DeliveryEstimate::new(
                    u16::try_from(min).map_err(corrupt)?,
                    u16::try_from(max).map_err(corrupt)?,
                )
                .map_err(corrupt)?,
            ),
            _ => None,
        };
        options
            .entry(col(&row, "zone_id")?)
            .or_default()
            .push(NewOption {
                id: ShippingOptionId::from_uuid(id),
                name: col(&row, "name")?,
                code: col(&row, "code")?,
                rate,
                estimate,
                tax_class: col::<Option<Uuid>>(&row, "tax_class_id")?.map(TaxClassId::from_uuid),
                is_backup: col(&row, "is_backup")?,
                enabled: col(&row, "enabled")?,
            });
    }

    zones
        .iter()
        .map(|row| {
            let id: Uuid = col(row, "id")?;
            let snapshot = ShippingZoneSnapshot {
                id: ShippingZoneId::from_uuid(id),
                version: version_from_db(col(row, "version")?)?,
                name: col(row, "name")?,
                countries: countries.remove(&id).unwrap_or_default(),
                options: options.remove(&id).unwrap_or_default(),
            };
            ShippingZone::restore(snapshot).map_err(|e| corrupt(format!("shipping zone {id}: {e}")))
        })
        .collect()
}

/// Groups an option's tier rows (in table, then tier order) back into tables.
fn builtin_rate(
    basis: RateBasis,
    rows: Vec<(i32, Currency, Tier)>,
) -> Result<BuiltinRate, RepoError> {
    let mut tables: Vec<(i32, Currency, Vec<Tier>)> = Vec::new();
    for (position, currency, tier) in rows {
        match tables.last_mut() {
            Some((p, _, tiers)) if *p == position => tiers.push(tier),
            _ => tables.push((position, currency, vec![tier])),
        }
    }
    let tables = tables
        .into_iter()
        .map(|(_, currency, tiers)| RateTable::new(currency, tiers))
        .collect::<Result<_, _>>()
        .map_err(corrupt)?;
    BuiltinRate::new(basis, tables).map_err(corrupt)
}

// ---------- app providers ----------

impl<C: PgConnection> AppProviderRepo for PgStore<C> {
    async fn get(&self, key: &ProviderKey) -> Result<Option<AppProvider>, RepoError> {
        let mut conn = self.conn().await;
        consistent_read!(conn.client(), |tx| {
            Ok(load_providers(tx, Some(key.as_str())).await?.pop())
        })
    }

    async fn list(&self) -> Result<Vec<AppProvider>, RepoError> {
        let mut conn = self.conn().await;
        consistent_read!(conn.client(), |tx| load_providers(tx, None).await)
    }

    async fn insert(&self, provider: &mut AppProvider) -> Result<(), RepoError> {
        let s = provider.snapshot();
        in_transaction!(self, |tx| insert_provider(tx, &s).await)?;
        provider.set_version(Version::NEW.next());
        Ok(())
    }

    async fn update(&self, provider: &mut AppProvider) -> Result<(), RepoError> {
        let s = provider.snapshot();
        let version = in_transaction!(self, |tx| update_provider(tx, &s).await)?;
        provider.set_version(version);
        Ok(())
    }

    async fn delete(&self, key: &ProviderKey) -> Result<(), RepoError> {
        in_transaction!(self, |tx| delete_provider(tx, key).await)
    }
}

/// `AppProviderRepo::insert` inside the caller's transaction: the row, then its services.
pub(crate) async fn insert_provider(
    tx: &impl GenericClient,
    s: &AppProviderSnapshot,
) -> Result<(), RepoError> {
    tx.query_typed(
        "INSERT INTO shipping_app_providers
            (key, version, name, callback_url, timeout_ms, secret, enabled)
         VALUES ($1, 1, $2, $3, $4, $5, $6)",
        &[
            (&s.key, Type::TEXT),
            (&s.name, Type::TEXT),
            (&s.callback_url, Type::TEXT),
            (&timeout_to_db(s.timeout_ms)?, Type::INT4),
            (&s.secret.as_bytes(), Type::BYTEA),
            (&s.enabled, Type::BOOL),
        ],
    )
    .await
    .map_err(repo_error)?;
    write_services(tx, s).await
}

/// `AppProviderRepo::update` inside the caller's transaction; the new version.
pub(crate) async fn update_provider(
    tx: &impl GenericClient,
    s: &AppProviderSnapshot,
) -> Result<Version, RepoError> {
    let bumped = tx
        .query_typed_opt(
            "UPDATE shipping_app_providers SET version = version + 1, name = $3,
                    callback_url = $4, timeout_ms = $5, secret = $6, enabled = $7
             WHERE key = $1 AND version = $2 RETURNING version",
            &[
                (&s.key, Type::TEXT),
                (&version_to_db(s.version)?, Type::INT8),
                (&s.name, Type::TEXT),
                (&s.callback_url, Type::TEXT),
                (&timeout_to_db(s.timeout_ms)?, Type::INT4),
                (&s.secret.as_bytes(), Type::BYTEA),
                (&s.enabled, Type::BOOL),
            ],
        )
        .await
        .map_err(repo_error)?;
    let row = bumped.ok_or(RepoError::StaleVersion)?;
    tx.query_typed(
        "DELETE FROM shipping_app_provider_services WHERE provider_key = $1",
        &[(&s.key, Type::TEXT)],
    )
    .await
    .map_err(repo_error)?;
    write_services(tx, s).await?;
    version_from_db(col(&row, "version")?)
}

/// `AppProviderRepo::delete` inside the caller's transaction.
pub(crate) async fn delete_provider(
    tx: &impl GenericClient,
    key: &ProviderKey,
) -> Result<(), RepoError> {
    tx.query_typed(
        "DELETE FROM shipping_app_providers WHERE key = $1",
        &[(&key.as_str(), Type::TEXT)],
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}

fn timeout_to_db(ms: u32) -> Result<i32, RepoError> {
    i32::try_from(ms).map_err(corrupt)
}

async fn write_services(tx: &impl GenericClient, s: &AppProviderSnapshot) -> Result<(), RepoError> {
    let codes: Vec<&str> = s.services.iter().map(|x| x.code.as_str()).collect();
    let names: Vec<&str> = s.services.iter().map(|x| x.name.as_str()).collect();
    tx.query_typed(
        "INSERT INTO shipping_app_provider_services (provider_key, position, code, name)
         SELECT $1, s.ord - 1, s.code, s.name
         FROM unnest($2::text[], $3::text[]) WITH ORDINALITY AS s(code, name, ord)",
        &[
            (&s.key, Type::TEXT),
            (&codes, Type::TEXT_ARRAY),
            (&names, Type::TEXT_ARRAY),
        ],
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}

/// The provider with this key (or all of them), by key, in two queries.
async fn load_providers(
    client: &impl GenericClient,
    key: Option<&str>,
) -> Result<Vec<AppProvider>, RepoError> {
    let rows = client
        .query_typed(
            "SELECT key, version, name, callback_url, timeout_ms, secret, enabled
             FROM shipping_app_providers WHERE $1::text IS NULL OR key = $1 ORDER BY key",
            &[(&key, Type::TEXT)],
        )
        .await
        .map_err(repo_error)?;
    let mut services: HashMap<String, Vec<ServiceInfo>> = HashMap::new();
    for row in client
        .query_typed(
            "SELECT provider_key, code, name FROM shipping_app_provider_services
             WHERE $1::text IS NULL OR provider_key = $1 ORDER BY provider_key, position",
            &[(&key, Type::TEXT)],
        )
        .await
        .map_err(repo_error)?
    {
        services
            .entry(col(&row, "provider_key")?)
            .or_default()
            .push(ServiceInfo {
                code: col(&row, "code")?,
                name: col(&row, "name")?,
            });
    }
    rows.iter()
        .map(|row| {
            let key: String = col(row, "key")?;
            let timeout: i32 = col(row, "timeout_ms")?;
            let snapshot = AppProviderSnapshot {
                services: services.remove(&key).unwrap_or_default(),
                key: key.clone(),
                version: version_from_db(col(row, "version")?)?,
                name: col(row, "name")?,
                callback_url: col(row, "callback_url")?,
                timeout_ms: u32::try_from(timeout).map_err(corrupt)?,
                secret: Sealed::from_bytes(col(row, "secret")?),
                enabled: col(row, "enabled")?,
            };
            AppProvider::restore(snapshot)
                .map_err(|e| corrupt(format!("shipping provider {key}: {e}")))
        })
        .collect()
}
