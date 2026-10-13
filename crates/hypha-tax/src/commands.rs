use hypha_core::{
    AuthError, CountryCode, CountryError, Ctx, ErrorCode, IdGenerator, MoneyError, ProductId,
    RepoError, Scope, TaxClassId, TaxRate, TaxRegionId,
};
use thiserror::Error;

use crate::{
    TaxClass, TaxError, TaxRegion,
    repo::{TaxClassRepo, TaxRegionRepo, keys},
};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CommandError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error(transparent)]
    Country(#[from] CountryError),
    #[error(transparent)]
    Money(#[from] MoneyError),
    #[error(transparent)]
    Tax(#[from] TaxError),
    #[error("tax class {0} not found")]
    TaxClassNotFound(TaxClassId),
    #[error("tax region {0} not found")]
    TaxRegionNotFound(TaxRegionId),
    #[error("country {country} already has tax region {region}")]
    TaxRegionExists {
        country: CountryCode,
        region: TaxRegionId,
    },
}

impl ErrorCode for CommandError {
    fn code(&self) -> &'static str {
        match self {
            CommandError::Auth(e) => e.code(),
            CommandError::Repo(e) => e.code(),
            CommandError::Country(e) => e.code(),
            CommandError::Money(e) => e.code(),
            CommandError::Tax(e) => e.code(),
            CommandError::TaxClassNotFound(_) => "tax_class_not_found",
            CommandError::TaxRegionNotFound(_) => "tax_region_not_found",
            CommandError::TaxRegionExists { .. } => "tax_region_exists",
        }
    }
}

pub struct CreateTaxClass {
    pub name: String,
}

pub async fn create_tax_class<C: TaxClassRepo, I: IdGenerator>(
    ctx: &Ctx,
    classes: &C,
    ids: &I,
    input: CreateTaxClass,
) -> Result<TaxClass, CommandError> {
    ctx.require(Scope::TaxesWrite)?;

    let mut class = TaxClass::new(TaxClassId::from_uuid(ids.new_uuid()), input.name);
    classes.insert(&mut class).await?;
    Ok(class)
}

pub struct AssignTaxClass {
    pub products: Vec<ProductId>,
    pub class: Option<TaxClassId>,
}

pub async fn assign_tax_class<C: TaxClassRepo>(
    ctx: &Ctx,
    classes: &C,
    input: AssignTaxClass,
) -> Result<(), CommandError> {
    ctx.require(Scope::TaxesWrite)?;

    if let Some(class) = input.class
        && classes.get(class).await?.is_none()
    {
        return Err(CommandError::TaxClassNotFound(class));
    }
    classes.assign(&input.products, input.class).await?;
    Ok(())
}

pub struct CreateTaxRegion {
    pub country: String,
    pub default_rate: u32,
}

pub async fn create_tax_region<R: TaxRegionRepo, I: IdGenerator>(
    ctx: &Ctx,
    regions: &R,
    ids: &I,
    input: CreateTaxRegion,
) -> Result<TaxRegion, CommandError> {
    ctx.require(Scope::TaxesWrite)?;

    let country = CountryCode::new(&input.country)?;
    let default_rate = TaxRate::new(input.default_rate)?;

    if let Some(existing) = regions.get_by_country(country).await? {
        return Err(region_exists(&existing));
    }

    let mut region = TaxRegion::new(
        TaxRegionId::from_uuid(ids.new_uuid()),
        country,
        default_rate,
    );
    match regions.insert(&mut region).await {
        Ok(()) => Ok(region),
        Err(RepoError::Duplicate(keys::TAX_REGION_COUNTRY)) => {
            match regions.get_by_country(country).await? {
                Some(existing) => Err(region_exists(&existing)),
                None => Err(RepoError::Duplicate(keys::TAX_REGION_COUNTRY).into()),
            }
        }
        Err(e) => Err(e.into()),
    }
}

fn region_exists(existing: &TaxRegion) -> CommandError {
    CommandError::TaxRegionExists {
        country: existing.country(),
        region: existing.id(),
    }
}

pub async fn get_tax_region<R: TaxRegionRepo>(
    ctx: &Ctx,
    regions: &R,
    id: TaxRegionId,
) -> Result<TaxRegion, CommandError> {
    ctx.require(Scope::TaxesRead)?;
    regions
        .get(id)
        .await?
        .ok_or(CommandError::TaxRegionNotFound(id))
}

pub async fn set_default_rate<R: TaxRegionRepo>(
    ctx: &Ctx,
    regions: &R,
    id: TaxRegionId,
    rate: u32,
) -> Result<TaxRegion, CommandError> {
    ctx.require(Scope::TaxesWrite)?;

    let rate = TaxRate::new(rate)?;
    let mut region = load_region(regions, id).await?;
    region.set_default_rate(rate);
    regions.update(&mut region).await?;
    Ok(region)
}

pub async fn set_class_rate<R: TaxRegionRepo, C: TaxClassRepo>(
    ctx: &Ctx,
    regions: &R,
    classes: &C,
    id: TaxRegionId,
    class: TaxClassId,
    rate: u32,
) -> Result<TaxRegion, CommandError> {
    ctx.require(Scope::TaxesWrite)?;

    let rate = TaxRate::new(rate)?;
    let mut region = load_region(regions, id).await?;
    if classes.get(class).await?.is_none() {
        return Err(CommandError::TaxClassNotFound(class));
    }
    region.set_class_rate(class, rate);
    regions.update(&mut region).await?;
    Ok(region)
}

pub async fn remove_class_rate<R: TaxRegionRepo>(
    ctx: &Ctx,
    regions: &R,
    id: TaxRegionId,
    class: TaxClassId,
) -> Result<TaxRegion, CommandError> {
    ctx.require(Scope::TaxesWrite)?;

    let mut region = load_region(regions, id).await?;
    region.remove_class_rate(class)?;
    regions.update(&mut region).await?;
    Ok(region)
}

async fn load_region<R: TaxRegionRepo>(
    regions: &R,
    id: TaxRegionId,
) -> Result<TaxRegion, CommandError> {
    regions
        .get(id)
        .await?
        .ok_or(CommandError::TaxRegionNotFound(id))
}
