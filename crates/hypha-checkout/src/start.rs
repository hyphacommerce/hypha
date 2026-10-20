//! Talking to a payment's provider (design notes 0027 §4, 0029 §3.1, 0031): starting a
//! payment (a cart's, before its order, for an online method; a manual method's, with
//! its order), asking where a started one is, and giving back money a checkout that
//! couldn't complete took.
//!
//! A provider that fails to start leaves the payment `pending`: the answer carries the
//! error, and starting again starts the same payment (the provider sees the same
//! idempotency key, the payment's ID).

use hypha_core::{
    Attempts, CartId, ChannelId, Clock, CountryCode, Email, MarketId, OrderId, PaymentId, RepoError,
};
use hypha_payments::{
    ActionAnswer, ActionRequest, HasPaymentMethods, HasPayments, NextAction, Payment,
    PaymentAction, PaymentError, PaymentMethodRepo, PaymentProviders, PaymentRepo, PaymentRequest,
    PaymentStatus, ProviderState, StatusRequest,
};

use crate::CommandError;

/// Who pays, as a provider is told: the cart, its order once there's one, and where and
/// to whom it sells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Payer {
    pub cart: CartId,
    /// The order and its number, for a manual method's payment (placed with it).
    pub order: Option<(OrderId, i64)>,
    pub channel: ChannelId,
    pub market: MarketId,
    pub country: CountryCode,
    pub email: Email,
}

/// Asks `payment`'s provider to start collecting it, with its method's `settings`, and
/// keeps the reference and attempt the provider answered with on the payment. The next
/// action, or why there's none (`Err`: the provider failed, the payment stays
/// `pending`).
///
/// Never fails otherwise: for a manual method it runs after the order is committed, so
/// a storage failure here must not turn a placed order into a checkout error. A
/// reference that can't be stored is logged and left out: the provider answers the same
/// one when the payment is started again (the same idempotency key, its ID), and its
/// reports carry it too.
pub(crate) async fn start<S: HasPayments, Q: PaymentProviders>(
    ports: &S,
    providers: &Q,
    payer: &Payer,
    payment: &mut Payment,
    settings: serde_json::Value,
    return_url: Option<&str>,
) -> Result<NextAction, PaymentError> {
    let Some(method) = payment.method() else {
        return Err(PaymentError::UnknownProvider(payment.provider().clone()));
    };
    let request = PaymentRequest {
        payment: payment.id(),
        cart: payer.cart,
        order: payer.order.map(|(order, _)| order),
        order_number: payer.order.map(|(_, number)| number),
        method,
        amount: payment.amount(),
        channel: payer.channel,
        market: payer.market,
        country: payer.country,
        customer_email: payer.email.clone(),
        return_url: return_url.map(str::to_string),
        reference: payment.reference().map(str::to_string),
        attempt: payment.attempt().map(str::to_string),
        instructions: payment.instructions().map(str::to_string),
        settings,
        // The host fills a spore provider's (design note 0030, §8).
        store_settings: serde_json::Map::new(),
    };
    let started = providers.start(payment.provider(), &request).await?;
    if started.reference.is_some() || started.attempt.is_some() {
        // As stored: the answer mustn't show a reference that wasn't kept.
        let before = payment.clone();
        if let Err(e) = keep_reference(ports, payment, started.reference, started.attempt).await {
            tracing::warn!(
                payment = %payment.id(),
                error = %e,
                "checkout: the provider's reference couldn't be stored; starting the payment again stores it"
            );
            *payment = before;
        }
    }
    Ok(started.next_action)
}

/// Stores the provider's `reference` on `payment`, and the `attempt` it started (made
/// again, the one reports are checked against from now), while the payment is still
/// pending: reloaded when the provider's own report (its events) changed it meanwhile.
/// Once it's moved on, the report's IDs stand.
async fn keep_reference<S: HasPayments>(
    ports: &S,
    payment: &mut Payment,
    reference: Option<String>,
    attempt: Option<String>,
) -> Result<(), CommandError> {
    let mut attempts = Attempts::first();
    loop {
        // Moved on meanwhile (its provider reported it): the report said where it stands.
        // The start's IDs may name an attempt that collected nothing: not written.
        if payment.status() != PaymentStatus::Pending {
            return Ok(());
        }
        let referenced = payment.set_reference(reference.clone())?;
        let started = payment.note_attempt(attempt.clone())?;
        if !(referenced || started) {
            return Ok(());
        }
        match ports.payments().update(payment).await {
            Err(RepoError::StaleVersion) if attempts.another() => {
                let id = payment.id();
                *payment = ports
                    .payments()
                    .get(id)
                    .await?
                    .ok_or_else(|| missing_payment(id))?;
            }
            done => return Ok(done?),
        }
    }
}

fn missing_payment(id: PaymentId) -> CommandError {
    // Payments aren't deleted.
    RepoError::Backend(format!("payment {id} is gone")).into()
}

/// A manual method's payment started again on a replay of checkout: its instructions,
/// from its method (none needed for `manual`, whose instructions are the payment's
/// copy).
pub(crate) async fn restart<S: HasPayments + HasPaymentMethods, Q: PaymentProviders>(
    ports: &S,
    providers: &Q,
    payer: &Payer,
    payment: &mut Payment,
) -> Result<Result<NextAction, PaymentError>, CommandError> {
    let settings = match payment.method() {
        Some(id) => ports
            .payment_methods()
            .get(id)
            .await?
            .map(|m| m.settings().clone()),
        None => None,
    };
    Ok(start(
        ports,
        providers,
        payer,
        payment,
        settings.unwrap_or(serde_json::Value::Null),
        None,
    )
    .await)
}

/// Asks the provider where a pending cart payment is (design note 0031, §4) and records
/// what it says on the payment: whether it moved. A provider that can't tell, or doesn't
/// answer, leaves it as it was (checkout then waits for its report: `payment_pending`).
pub(crate) async fn refresh<S: HasPayments, Q: PaymentProviders>(
    ports: &S,
    providers: &Q,
    clock: &impl Clock,
    payment: &mut Payment,
) -> Result<bool, CommandError> {
    let request = StatusRequest {
        payment: payment.id(),
        cart: payment.cart(),
        reference: payment.reference().map(str::to_string),
        attempt: payment.attempt().map(str::to_string),
        store_settings: serde_json::Map::new(),
    };
    let status = match providers.status(payment.provider(), &request).await {
        Ok(status) => status,
        Err(e) => {
            tracing::info!(payment = %payment.id(), error = %e, "checkout: the provider didn't say where the payment is");
            return Ok(false);
        }
    };
    // Only the attempt the payment collects through moves it (another one's state isn't
    // this payment's: design note 0030).
    if let (Some(held), Some(said)) = (payment.attempt(), status.attempt.as_deref())
        && held != said
    {
        return Ok(false);
    }
    let reference = status.reference.clone();
    let moved = match status.state {
        ProviderState::Open => false,
        ProviderState::Processing => payment.process(reference)?,
        ProviderState::Authorized => payment.authorize(reference)?,
        ProviderState::Paid => payment.pay(clock.now(), reference)?,
        ProviderState::Failed => payment.fail(reference)?,
        ProviderState::Canceled => payment.void(clock.now())?,
    };
    if moved {
        payment.note_attempt(status.attempt)?;
        ports.payments().update(payment).await?;
    }
    Ok(moved)
}

/// Gives back what a cart's committed `payment` holds, when checkout couldn't place its
/// order (design note 0031, §3): an authorized one voided, a paid one refunded in full,
/// through its provider; what the provider says it did is recorded at once. A
/// `processing` one can't be yet (logged for staff). Never fails: the shopper's error is
/// what the caller answers, and the provider's own report records the rest.
pub(crate) async fn release<S: HasPayments, Q: PaymentProviders>(
    ports: &S,
    providers: &Q,
    clock: &impl Clock,
    mut payment: Payment,
) {
    let action = match payment.status() {
        PaymentStatus::Authorized => PaymentAction::Void,
        PaymentStatus::Paid => PaymentAction::Refund,
        PaymentStatus::Processing => {
            tracing::warn!(payment = %payment.id(), "checkout: couldn't place the order, and the payment is processing: give it back from the provider's dashboard once it's collected");
            return;
        }
        _ => return,
    };
    let request = ActionRequest {
        action,
        payment: payment.id(),
        cart: payment.cart(),
        order: None,
        amount: None,
        reference: payment.reference().map(str::to_string),
        attempt: payment.attempt().map(str::to_string),
        idempotency_key: format!("{}:{action}:checkout", payment.id()),
        store_settings: serde_json::Map::new(),
    };
    let answer = providers.act(payment.provider(), &request).await;
    let recorded = match answer {
        Ok(ActionAnswer::Completed { reference }) => match action {
            PaymentAction::Void => payment.void(clock.now()).map(|_| ()),
            _ => payment
                .refund(None, PaymentStatus::Refunded, reference)
                .map(|_| ()),
        },
        // The provider's report records it.
        Ok(ActionAnswer::Accepted { .. }) => return,
        Ok(ActionAnswer::Refused { reason }) => {
            tracing::warn!(payment = %payment.id(), %reason, "checkout: couldn't place the order, and the provider refused to give the money back");
            return;
        }
        Err(e) => {
            tracing::warn!(payment = %payment.id(), error = %e, "checkout: couldn't place the order, and couldn't give the money back");
            return;
        }
    };
    let stored = match recorded {
        Ok(()) => ports
            .payments()
            .update(&mut payment)
            .await
            .map_err(|e| e.to_string()),
        Err(e) => Err(e.to_string()),
    };
    if let Err(e) = stored {
        // The provider's report (its webhook) records it.
        tracing::warn!(payment = %payment.id(), error = %e, "checkout: gave the money back, but couldn't record it");
    }
}

/// Asks the provider to release a cart's pending payment that a new one replaced
/// (voided in Hypha already): best effort, so a shopper paying in an old tab is declined
/// rather than charged. Money it takes anyway is a second collection, given back.
pub(crate) async fn release_pending<Q: PaymentProviders>(providers: &Q, payment: Payment) {
    let request = ActionRequest {
        action: PaymentAction::Void,
        payment: payment.id(),
        cart: payment.cart(),
        order: None,
        amount: None,
        reference: payment.reference().map(str::to_string),
        attempt: payment.attempt().map(str::to_string),
        idempotency_key: format!("{}:void:replaced", payment.id()),
        store_settings: serde_json::Map::new(),
    };
    match providers.act(payment.provider(), &request).await {
        Ok(_) | Err(PaymentError::ActionUnsupported { .. }) => {}
        Err(e) => {
            tracing::info!(payment = %payment.id(), error = %e, "checkout: couldn't release a replaced payment at its provider");
        }
    }
}
