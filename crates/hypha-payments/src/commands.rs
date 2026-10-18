//! Managing the payment methods a store offers (design note 0027, §7): settings, so
//! `payments:*`. Recording payments on orders is the workflows' (`hypha-order-actions`,
//! `hypha-checkout`).

use hypha_core::{
    AuthError, ChannelId, Ctx, ErrorCode, IdGenerator, MarketId, PaymentMethodId, RepoError, Scope,
};
use thiserror::Error;

use crate::{
    HasPaymentMethods, NewPaymentMethod, PaymentError, PaymentMethod, PaymentMethodRepo,
    PaymentProviders, ProviderKey, keys,
};

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum CommandError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error(transparent)]
    Payment(#[from] PaymentError),
    #[error("payment method {0} not found")]
    PaymentMethodNotFound(PaymentMethodId),
    /// Deleting a method an open (pending or authorized) payment uses: disable it
    /// instead.
    #[error("payment method {0} is used by an open payment; disable it instead")]
    PaymentMethodInUse(PaymentMethodId),
}

impl ErrorCode for CommandError {
    fn code(&self) -> &'static str {
        match self {
            CommandError::Auth(e) => e.code(),
            CommandError::Repo(e) => e.code(),
            CommandError::Payment(e) => e.code(),
            CommandError::PaymentMethodNotFound(_) => "payment_method_not_found",
            CommandError::PaymentMethodInUse(_) => "payment_method_in_use",
        }
    }
}

async fn load<R: PaymentMethodRepo>(
    repo: &R,
    id: PaymentMethodId,
) -> Result<PaymentMethod, CommandError> {
    repo.get(id)
        .await?
        .ok_or(CommandError::PaymentMethodNotFound(id))
}

/// A provider this build has, with `settings` it accepts.
fn check_provider<P: PaymentProviders>(
    providers: &P,
    provider: &ProviderKey,
    settings: &serde_json::Value,
) -> Result<(), PaymentError> {
    if !providers.has(provider) {
        return Err(PaymentError::UnknownProvider(provider.clone()));
    }
    providers.check_settings(provider, settings)
}

/// Every method, in the order shoppers see them (position, then ID).
pub async fn list_payment_methods<S: HasPaymentMethods>(
    ctx: &Ctx,
    ports: &S,
) -> Result<Vec<PaymentMethod>, CommandError> {
    ctx.require(Scope::PaymentsRead)?;
    Ok(ports.payment_methods().list().await?)
}

pub async fn get_payment_method<S: HasPaymentMethods>(
    ctx: &Ctx,
    ports: &S,
    id: PaymentMethodId,
) -> Result<PaymentMethod, CommandError> {
    ctx.require(Scope::PaymentsRead)?;
    load(ports.payment_methods(), id).await
}

/// A new method as a client sends it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatePaymentMethod {
    /// `manual`, or a spore's `spore_<name>`.
    pub provider: String,
    pub name: String,
    pub instructions: Option<String>,
    /// Empty: every market. Not checked against markets (a leaf can't see them): an
    /// unknown one never matches.
    pub markets: Vec<MarketId>,
    /// Empty: every channel; unchecked like markets.
    pub channels: Vec<ChannelId>,
    pub enabled: bool,
    /// `None`: after every method there is.
    pub position: Option<u32>,
    /// The provider's settings; `None` is `{}`.
    pub settings: Option<serde_json::Value>,
}

/// Creates a method for a provider this build has (`payment_provider_unknown`
/// otherwise), with settings the provider accepts (`invalid_payment_settings`).
pub async fn create_payment_method<S: HasPaymentMethods, P: PaymentProviders>(
    ctx: &Ctx,
    ports: &S,
    ids: &impl IdGenerator,
    providers: &P,
    input: CreatePaymentMethod,
) -> Result<PaymentMethod, CommandError> {
    ctx.require(Scope::PaymentsWrite)?;
    let provider = ProviderKey::new(&input.provider)?;
    let settings = input
        .settings
        .unwrap_or_else(|| serde_json::Value::Object(serde_json::Map::new()));
    check_provider(providers, &provider, &settings)?;
    let position = match input.position {
        Some(position) => position,
        None => ports
            .payment_methods()
            .list()
            .await?
            .iter()
            .map(|m| m.position().saturating_add(1))
            .max()
            .unwrap_or(0),
    };
    let mut method = PaymentMethod::new(NewPaymentMethod {
        id: PaymentMethodId::from_uuid(ids.new_uuid()),
        provider,
        name: input.name,
        instructions: input.instructions,
        markets: input.markets,
        channels: input.channels,
        enabled: input.enabled,
        position,
        settings,
    })?;
    ports.payment_methods().insert(&mut method).await?;
    Ok(method)
}

/// A JSON Merge Patch of a method: `None` keeps a field; `instructions: Some(None)`
/// clears them. The provider can't change.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UpdatePaymentMethod {
    pub name: Option<String>,
    pub instructions: Option<Option<String>>,
    pub markets: Option<Vec<MarketId>>,
    pub channels: Option<Vec<ChannelId>>,
    pub enabled: Option<bool>,
    /// Where it shows among the others (reordering is setting positions).
    pub position: Option<u32>,
    pub settings: Option<serde_json::Value>,
}

/// Changes a method. Payments made with it keep the name and instructions they copied.
pub async fn update_payment_method<S: HasPaymentMethods, P: PaymentProviders>(
    ctx: &Ctx,
    ports: &S,
    providers: &P,
    id: PaymentMethodId,
    input: UpdatePaymentMethod,
) -> Result<PaymentMethod, CommandError> {
    ctx.require(Scope::PaymentsWrite)?;
    let mut method = load(ports.payment_methods(), id).await?;
    if let Some(name) = &input.name {
        method.rename(name)?;
    }
    if let Some(instructions) = input.instructions {
        method.set_instructions(instructions)?;
    }
    if let Some(markets) = input.markets {
        method.set_markets(markets);
    }
    if let Some(channels) = input.channels {
        method.set_channels(channels);
    }
    if let Some(enabled) = input.enabled {
        method.set_enabled(enabled);
    }
    if let Some(position) = input.position {
        method.set_position(position);
    }
    if let Some(settings) = input.settings {
        check_provider(providers, method.provider(), &settings)?;
        method.set_settings(settings);
    }
    ports.payment_methods().update(&mut method).await?;
    Ok(method)
}

/// Deletes a method no open (pending or authorized) payment uses
/// (`payment_method_in_use` otherwise: disable it instead). The check and the delete are
/// one step in storage, so a checkout starting a payment with it at the same moment
/// can't be stranded. Payments it made keep its name and provider.
pub async fn delete_payment_method<S: HasPaymentMethods>(
    ctx: &Ctx,
    ports: &S,
    id: PaymentMethodId,
) -> Result<(), CommandError> {
    ctx.require(Scope::PaymentsWrite)?;
    let method = load(ports.payment_methods(), id).await?;
    match ports.payment_methods().delete(&method).await {
        Err(RepoError::Reference(keys::OPEN_PAYMENT_METHOD)) => {
            Err(CommandError::PaymentMethodInUse(id))
        }
        done => Ok(done?),
    }
}
