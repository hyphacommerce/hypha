//! Managing the payment methods a store offers: settings, so `payments:*`; and asking a
//! payment's provider to capture, void or refund it. Recording payments on orders is the
//! workflows' (`hypha-order-actions`, `hypha-checkout`).

use hypha_core::{
    AuthError, ChannelId, Ctx, ErrorCode, IdGenerator, MarketId, Money, PaymentId, PaymentMethodId,
    RepoError, Scope,
};
use thiserror::Error;

use crate::{
    ActionAnswer, ActionRequest, HasPaymentMethods, HasPayments, NewPaymentMethod, Payment,
    PaymentAction, PaymentError, PaymentMethod, PaymentMethodRepo, PaymentProviders, PaymentRepo,
    PaymentStatus, ProviderKey, keys,
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
    #[error("payment {0} not found")]
    PaymentNotFound(PaymentId),
    /// The provider said no to a capture, void or refund; `reason` is its words.
    #[error("payment {payment}: the provider refused: {reason}")]
    PaymentRefused { payment: PaymentId, reason: String },
}

impl ErrorCode for CommandError {
    fn code(&self) -> &'static str {
        match self {
            CommandError::Auth(e) => e.code(),
            CommandError::Repo(e) => e.code(),
            CommandError::Payment(e) => e.code(),
            CommandError::PaymentMethodNotFound(_) => "payment_method_not_found",
            CommandError::PaymentMethodInUse(_) => "payment_method_in_use",
            CommandError::PaymentNotFound(_) => "payment_not_found",
            CommandError::PaymentRefused { .. } => "payment_refused",
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

/// A provider the host offers (one this build has, or an installed, enabled app
/// implementing `payments.start`), with `settings` it accepts.
async fn check_provider<P: PaymentProviders>(
    providers: &P,
    provider: &ProviderKey,
    settings: &serde_json::Value,
) -> Result<(), CommandError> {
    if !providers.available(provider).await? {
        return Err(PaymentError::UnknownProvider(provider.clone()).into());
    }
    Ok(providers.check_settings(provider, settings)?)
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

/// One payment, as the order shows it (`orders:read`: reading an order's payments is
/// reading the order). A payment app looks up the payment it was asked to start or act on.
pub async fn get_payment<S: HasPayments>(
    ctx: &Ctx,
    ports: &S,
    id: PaymentId,
) -> Result<Payment, CommandError> {
    ctx.require(Scope::OrdersRead)?;
    ports
        .payments()
        .get(id)
        .await?
        .ok_or(CommandError::PaymentNotFound(id))
}

/// A new method as a client sends it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatePaymentMethod {
    /// `manual`, or an app's key `app_<name>` (installed, enabled and implementing
    /// `payments.start`).
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

/// Creates a method for a provider the host offers (`payment_provider_unknown`
/// otherwise: an app must be installed, enabled and implement `payments.start`), with
/// settings the provider accepts (`invalid_payment_settings`; an app keeps its own, so
/// it takes none).
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
    check_provider(providers, &provider, &settings).await?;
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
        check_provider(providers, method.provider(), &settings).await?;
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

/// Staff asking a payment's provider to capture, void or refund it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActOnPayment {
    pub payment: PaymentId,
    pub action: PaymentAction,
    /// For a refund, how much in the payment's minor units; `None` for all that's left.
    /// Captures and voids are of the whole payment.
    pub amount: Option<i64>,
    /// The caller's `Idempotency-Key`, passed on to the provider so a retried action is
    /// done once. Without one, each attempt gets a new key (`<payment>:<action>:<id>`):
    /// a key per payment would make the provider replay a refused attempt's answer to
    /// every later one.
    pub idempotency_key: Option<String>,
}

/// The provider accepted: the payment as it still is (it moves when the provider
/// reports the result, `hypha_order_actions::record_payment_event`), and the
/// provider's reference for the action, when it gave one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Acted {
    pub payment: Payment,
    pub reference: Option<String>,
    /// The provider says it's done already (`ActionAnswer::Completed`): the caller
    /// records it (`hypha_order_actions::record_payment_event`).
    pub completed: bool,
}

/// Asks `input.payment`'s provider to capture, void or refund it (`orders:write`: what
/// staff do on an order). Checked first against the payment's status (a capture needs
/// `authorized`, a void `pending` or `authorized`, a refund `paid` or
/// `partially_refunded`: `invalid_payment_transition`) and, for a refund, what's left
/// (`invalid_refund_amount`). The provider's no is `payment_refused` with its reason;
/// `manual` takes no actions (`payment_action_unsupported`); an app that gives no
/// usable answer is `app_unavailable`. Accepted changes nothing yet (as Shopify's
/// payments apps do): the provider reports what happened through the payment's events.
pub async fn act_on_payment<S: HasPayments, P: PaymentProviders>(
    ctx: &Ctx,
    ports: &S,
    ids: &impl IdGenerator,
    providers: &P,
    input: ActOnPayment,
) -> Result<Acted, CommandError> {
    ctx.require(Scope::OrdersWrite)?;
    let payment = ports
        .payments()
        .get(input.payment)
        .await?
        .ok_or(CommandError::PaymentNotFound(input.payment))?;
    let currency = payment.amount().currency();
    let left = payment.amount().amount() - payment.refunded().amount();
    let (to, amount) = match input.action {
        PaymentAction::Capture => (PaymentStatus::Paid, None),
        PaymentAction::Void => (PaymentStatus::Voided, None),
        PaymentAction::Refund => {
            let amount = input.amount.unwrap_or(left);
            let to = if amount < left {
                PaymentStatus::PartiallyRefunded
            } else {
                PaymentStatus::Refunded
            };
            (to, Some(amount))
        }
    };
    // Capture asks the provider for money it holds: `authorized`, or `pending` when the
    // authorization hasn't been reported yet (a provider's webhook that was late or lost:
    // the provider knows better than our copy, and refuses when there's nothing to take).
    if !payment.status().can_become(to) {
        return Err(PaymentError::InvalidTransition {
            payment: payment.id(),
            from: payment.status(),
            to,
        }
        .into());
    }
    if let Some(amount) = amount
        && (amount <= 0 || amount > left)
    {
        return Err(PaymentError::InvalidRefund {
            payment: payment.id(),
            amount,
            paid: payment.amount().amount(),
            refunded: payment.refunded().amount(),
            reported: to,
        }
        .into());
    }
    let idempotency_key = input
        .idempotency_key
        .unwrap_or_else(|| format!("{}:{}:{}", payment.id(), input.action, ids.new_uuid()));
    let request = ActionRequest {
        action: input.action,
        payment: payment.id(),
        cart: payment.cart(),
        order: payment.order(),
        amount: amount
            .map(|a| Money::new(a, currency))
            .transpose()
            .map_err(PaymentError::from)?,
        reference: payment.reference().map(str::to_string),
        attempt: payment.attempt().map(str::to_string),
        idempotency_key,
        // The host fills a spore provider's.
        store_settings: serde_json::Map::new(),
    };
    match providers.act(payment.provider(), &request).await? {
        ActionAnswer::Accepted { reference } => Ok(Acted {
            payment,
            reference,
            completed: false,
        }),
        ActionAnswer::Completed { reference } => Ok(Acted {
            payment,
            reference,
            completed: true,
        }),
        ActionAnswer::Refused { reason } => Err(CommandError::PaymentRefused {
            payment: payment.id(),
            reason,
        }),
    }
}
