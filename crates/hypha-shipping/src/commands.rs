//! The shipping setup a merchant (or an admin agent) manages: zones and their options,
//! and the external apps that calculate rates (design note 0015, section 8).

use hypha_core::{
    AuthError, CountryCode, CountryError, Ctx, ErrorCode, IdGenerator, Random, RepoError, Scope,
    SealError, SecretBox, ShippingOptionId, ShippingZoneId, TaxClassId, report_duplicate,
};
use thiserror::Error;

use crate::{
    AppProvider, AppProviderRepo, DeliveryEstimate, NewAppProvider, NewOption, OptionPatch,
    OptionRate, ProviderError, ProviderKey, RateProvider, ServiceInfo, ShippingError, ShippingZone,
    ShippingZoneRepo, keys,
};

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum CommandError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Shipping(#[from] ShippingError),
    #[error(transparent)]
    Country(#[from] CountryError),
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error(transparent)]
    Provider(#[from] ProviderError),
    #[error("shipping zone {0} not found")]
    ZoneNotFound(ShippingZoneId),
    #[error("{country} is already in shipping zone {zone}")]
    CountryInOtherZone {
        country: CountryCode,
        zone: ShippingZoneId,
    },
    #[error("shipping provider {0} not found")]
    ProviderNotFound(ProviderKey),
    #[error("shipping provider {0} already exists")]
    ProviderExists(ProviderKey),
    /// The stored secret can't be opened (another `HYPHA_SECRET_KEY`): a server fault.
    #[error(transparent)]
    Seal(#[from] SealError),
}

impl ErrorCode for CommandError {
    fn code(&self) -> &'static str {
        match self {
            CommandError::Auth(e) => e.code(),
            CommandError::Shipping(e) => e.code(),
            CommandError::Country(e) => e.code(),
            CommandError::Repo(e) => e.code(),
            CommandError::Provider(e) => e.code(),
            CommandError::ZoneNotFound(_) => "shipping_zone_not_found",
            CommandError::CountryInOtherZone { .. } => "country_in_other_zone",
            CommandError::ProviderNotFound(_) => "shipping_provider_not_found",
            CommandError::ProviderExists(_) => "shipping_provider_exists",
            CommandError::Seal(_) => "seal_failed",
        }
    }
}

// ---------- zones ----------

pub struct CreateZone {
    pub name: String,
    /// ISO 3166-1 alpha-2 codes.
    pub countries: Vec<String>,
}

pub async fn create_zone<R: ShippingZoneRepo>(
    ctx: &Ctx,
    zones: &R,
    ids: &impl IdGenerator,
    input: CreateZone,
) -> Result<ShippingZone, CommandError> {
    ctx.require(Scope::ShippingWrite)?;
    let countries = parse_countries(&input.countries)?;
    let mut zone = ShippingZone::new(
        ShippingZoneId::from_uuid(ids.new_uuid()),
        &input.name,
        countries,
    )?;
    check_countries_free(zones, &zone).await?;
    let written = zones.insert(&mut zone).await;
    report_duplicate(written, keys::ZONE_COUNTRY, country_taken(zones, &zone)).await?;
    Ok(zone)
}

pub async fn get_zone<R: ShippingZoneRepo>(
    ctx: &Ctx,
    zones: &R,
    id: ShippingZoneId,
) -> Result<ShippingZone, CommandError> {
    ctx.require(Scope::ShippingRead)?;
    load_zone(zones, id).await
}

pub async fn list_zones<R: ShippingZoneRepo>(
    ctx: &Ctx,
    zones: &R,
) -> Result<Vec<ShippingZone>, CommandError> {
    ctx.require(Scope::ShippingRead)?;
    Ok(zones.list().await?)
}

/// Changes only the fields that are `Some`.
#[derive(Default)]
pub struct UpdateZone {
    pub name: Option<String>,
    pub countries: Option<Vec<String>>,
}

pub async fn update_zone<R: ShippingZoneRepo>(
    ctx: &Ctx,
    zones: &R,
    id: ShippingZoneId,
    input: UpdateZone,
) -> Result<ShippingZone, CommandError> {
    ctx.require(Scope::ShippingWrite)?;
    let countries = input
        .countries
        .as_deref()
        .map(parse_countries)
        .transpose()?;
    let mut zone = load_zone(zones, id).await?;
    if let Some(name) = &input.name {
        zone.rename(name)?;
    }
    if let Some(countries) = countries {
        zone.set_countries(countries)?;
        check_countries_free(zones, &zone).await?;
    }
    let written = zones.update(&mut zone).await;
    report_duplicate(written, keys::ZONE_COUNTRY, country_taken(zones, &zone)).await?;
    Ok(zone)
}

/// Removes a zone and its options. A cart that picked one of them finds it gone at
/// checkout (`shipping_option_unavailable`) and picks again.
pub async fn delete_zone<R: ShippingZoneRepo>(
    ctx: &Ctx,
    zones: &R,
    id: ShippingZoneId,
) -> Result<(), CommandError> {
    ctx.require(Scope::ShippingWrite)?;
    load_zone(zones, id).await?;
    Ok(zones.delete(id).await?)
}

// ---------- options ----------

pub struct AddOption {
    pub name: String,
    pub code: String,
    pub rate: OptionRate,
    pub estimate: Option<DeliveryEstimate>,
    pub tax_class: Option<TaxClassId>,
    pub is_backup: bool,
    pub enabled: bool,
}

pub async fn add_option<R: ShippingZoneRepo>(
    ctx: &Ctx,
    zones: &R,
    ids: &impl IdGenerator,
    zone: ShippingZoneId,
    input: AddOption,
) -> Result<ShippingZone, CommandError> {
    ctx.require(Scope::ShippingWrite)?;
    let mut zone = load_zone(zones, zone).await?;
    zone.add_option(NewOption {
        id: ShippingOptionId::from_uuid(ids.new_uuid()),
        name: input.name,
        code: input.code,
        rate: input.rate,
        estimate: input.estimate,
        tax_class: input.tax_class,
        is_backup: input.is_backup,
        enabled: input.enabled,
    })?;
    zones.update(&mut zone).await?;
    Ok(zone)
}

pub async fn update_option<R: ShippingZoneRepo>(
    ctx: &Ctx,
    zones: &R,
    zone: ShippingZoneId,
    option: ShippingOptionId,
    patch: OptionPatch,
) -> Result<ShippingZone, CommandError> {
    ctx.require(Scope::ShippingWrite)?;
    let mut zone = load_zone(zones, zone).await?;
    zone.update_option(option, patch)?;
    zones.update(&mut zone).await?;
    Ok(zone)
}

pub async fn remove_option<R: ShippingZoneRepo>(
    ctx: &Ctx,
    zones: &R,
    zone: ShippingZoneId,
    option: ShippingOptionId,
) -> Result<ShippingZone, CommandError> {
    ctx.require(Scope::ShippingWrite)?;
    let mut zone = load_zone(zones, zone).await?;
    zone.remove_option(option)?;
    zones.update(&mut zone).await?;
    Ok(zone)
}

pub async fn reorder_options<R: ShippingZoneRepo>(
    ctx: &Ctx,
    zones: &R,
    zone: ShippingZoneId,
    order: Vec<ShippingOptionId>,
) -> Result<ShippingZone, CommandError> {
    ctx.require(Scope::ShippingWrite)?;
    let mut zone = load_zone(zones, zone).await?;
    zone.reorder_options(&order)?;
    zones.update(&mut zone).await?;
    Ok(zone)
}

fn parse_countries(codes: &[String]) -> Result<Vec<CountryCode>, CommandError> {
    Ok(codes
        .iter()
        .map(|c| CountryCode::new(c))
        .collect::<Result<_, _>>()?)
}

async fn load_zone<R: ShippingZoneRepo>(
    zones: &R,
    id: ShippingZoneId,
) -> Result<ShippingZone, CommandError> {
    zones.get(id).await?.ok_or(CommandError::ZoneNotFound(id))
}

/// `CountryInOtherZone` for the first of `zone`'s countries another zone has.
async fn country_taken<R: ShippingZoneRepo>(
    zones: &R,
    zone: &ShippingZone,
) -> Result<Option<CommandError>, CommandError> {
    for country in zone.countries() {
        if let Some(other) = zones.get_by_country(*country).await?
            && other.id() != zone.id()
        {
            return Ok(Some(CommandError::CountryInOtherZone {
                country: *country,
                zone: other.id(),
            }));
        }
    }
    Ok(None)
}

async fn check_countries_free<R: ShippingZoneRepo>(
    zones: &R,
    zone: &ShippingZone,
) -> Result<(), CommandError> {
    match country_taken(zones, zone).await? {
        Some(taken) => Err(taken),
        None => Ok(()),
    }
}

// ---------- app providers ----------

/// The prefix of callback signing secrets: recognisable in logs and secret scanners.
pub const SECRET_PREFIX: &str = "hypha_shs_";

pub struct CreateAppProvider {
    /// `app_<name>`.
    pub key: String,
    pub name: String,
    pub callback_url: String,
    pub timeout_ms: Option<u32>,
    /// Services the app offers, if it lists them up front.
    pub services: Vec<ServiceInfo>,
}

/// Registers an external app that calculates rates, and returns it with its signing
/// secret. The secret is shown this once; Hypha keeps it sealed.
pub async fn create_app_provider<R: AppProviderRepo>(
    ctx: &Ctx,
    providers: &R,
    random: &impl Random,
    secrets: &impl SecretBox,
    input: CreateAppProvider,
) -> Result<(AppProvider, String), CommandError> {
    ctx.require(Scope::ShippingWrite)?;
    let key = ProviderKey::new(&input.key)?;
    let (secret, sealed) = new_secret(&key, random, secrets);
    let mut provider = AppProvider::new(NewAppProvider {
        key: key.clone(),
        name: input.name,
        callback_url: input.callback_url,
        timeout_ms: input.timeout_ms.unwrap_or(AppProvider::DEFAULT_TIMEOUT_MS),
        secret: sealed,
        services: input.services,
    })?;
    if providers.get(&key).await?.is_some() {
        return Err(CommandError::ProviderExists(key));
    }
    match providers.insert(&mut provider).await {
        Err(RepoError::Duplicate(k)) if k == keys::PROVIDER_KEY => {
            Err(CommandError::ProviderExists(key))
        }
        written => {
            written?;
            Ok((provider, secret))
        }
    }
}

pub async fn get_app_provider<R: AppProviderRepo>(
    ctx: &Ctx,
    providers: &R,
    key: &str,
) -> Result<AppProvider, CommandError> {
    ctx.require(Scope::ShippingRead)?;
    load_provider(providers, key).await
}

pub async fn list_app_providers<R: AppProviderRepo>(
    ctx: &Ctx,
    providers: &R,
) -> Result<Vec<AppProvider>, CommandError> {
    ctx.require(Scope::ShippingRead)?;
    Ok(providers.list().await?)
}

/// Changes only the fields that are `Some`.
#[derive(Default)]
pub struct UpdateAppProvider {
    pub name: Option<String>,
    pub callback_url: Option<String>,
    pub timeout_ms: Option<u32>,
    pub services: Option<Vec<ServiceInfo>>,
    pub enabled: Option<bool>,
}

pub async fn update_app_provider<R: AppProviderRepo>(
    ctx: &Ctx,
    providers: &R,
    key: &str,
    input: UpdateAppProvider,
) -> Result<AppProvider, CommandError> {
    ctx.require(Scope::ShippingWrite)?;
    let mut provider = load_provider(providers, key).await?;
    if let Some(name) = &input.name {
        provider.rename(name)?;
    }
    if let Some(url) = &input.callback_url {
        provider.set_callback_url(url)?;
    }
    if let Some(timeout) = input.timeout_ms {
        provider.set_timeout_ms(timeout)?;
    }
    if let Some(services) = input.services {
        provider.set_services(services)?;
    }
    if let Some(enabled) = input.enabled {
        provider.set_enabled(enabled);
    }
    providers.update(&mut provider).await?;
    Ok(provider)
}

/// Replaces the signing secret and returns the new one, shown this once. Calls signed
/// with the old one stop verifying at once.
pub async fn rotate_app_provider_secret<R: AppProviderRepo>(
    ctx: &Ctx,
    providers: &R,
    random: &impl Random,
    secrets: &impl SecretBox,
    key: &str,
) -> Result<(AppProvider, String), CommandError> {
    ctx.require(Scope::ShippingWrite)?;
    let mut provider = load_provider(providers, key).await?;
    let (secret, sealed) = new_secret(provider.key(), random, secrets);
    provider.set_secret(sealed);
    providers.update(&mut provider).await?;
    Ok((provider, secret))
}

/// Removes an app provider. Options that name it stop being offered
/// (`provider_failed`: unknown provider) until they're changed.
pub async fn delete_app_provider<R: AppProviderRepo>(
    ctx: &Ctx,
    providers: &R,
    key: &str,
) -> Result<(), CommandError> {
    ctx.require(Scope::ShippingWrite)?;
    let provider = load_provider(providers, key).await?;
    Ok(providers.delete(provider.key()).await?)
}

/// The services a provider (app or spore) can quote, for the option editor.
pub async fn provider_services<P: RateProvider>(
    ctx: &Ctx,
    rates: &P,
    key: &str,
) -> Result<Vec<ServiceInfo>, CommandError> {
    ctx.require(Scope::ShippingRead)?;
    let key = ProviderKey::new(key)?;
    Ok(rates.services(&key).await?)
}

/// The signing secret in plain text, for the host's callback client. Never returned by
/// an API: only `create_app_provider` and `rotate_app_provider_secret` show it.
pub fn open_secret(secrets: &impl SecretBox, provider: &AppProvider) -> Result<String, SealError> {
    let plain = secrets.open(provider.secret(), provider.key().as_str().as_bytes())?;
    String::from_utf8(plain).map_err(|_| SealError("secret isn't UTF-8"))
}

/// 32 random bytes as hex after the prefix, and that string sealed with the key as context.
fn new_secret(
    key: &ProviderKey,
    random: &impl Random,
    secrets: &impl SecretBox,
) -> (String, hypha_core::Sealed) {
    let mut bytes = [0u8; 32];
    random.fill(&mut bytes);
    let mut secret = String::with_capacity(SECRET_PREFIX.len() + 64);
    secret.push_str(SECRET_PREFIX);
    for b in bytes {
        secret.push_str(&format!("{b:02x}"));
    }
    let mut nonce = [0u8; 24];
    random.fill(&mut nonce);
    let sealed = secrets.seal(secret.as_bytes(), key.as_str().as_bytes(), nonce);
    (secret, sealed)
}

async fn load_provider<R: AppProviderRepo>(
    providers: &R,
    key: &str,
) -> Result<AppProvider, CommandError> {
    let key = ProviderKey::new(key)?;
    providers
        .get(&key)
        .await?
        .ok_or(CommandError::ProviderNotFound(key))
}
