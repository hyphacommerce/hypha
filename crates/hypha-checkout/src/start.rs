//! Asking a payment's provider to start collecting it (design note 0027, §4; 0029,
//! §3.1), after checkout committed it, on a replay of checkout, and when the shopper
//! asks again (`start_order_payment`, `POST /storefront/orders/{id}/payment`).
//!
//! A provider that fails leaves the order placed and the payment `pending` (design note
//! 0027, §4): the answer carries the error, and asking again starts the same payment
//! (the provider sees the same idempotency key, the payment's ID), or, once the provider
//! reported it `failed`, a new one.

use hypha_core::{
    Attempts, Ctx, FinancialStatus, IdGenerator, OrderId, PaymentId, RepoError, Scope,
};
use hypha_orders::{Order, OrderError, OrderRepo, StageOrders};
use hypha_payments::{
    HasPaymentMethods, HasPayments, NewPayment, NextAction, Payment, PaymentError,
    PaymentMethodRepo, PaymentProviders, PaymentRepo, PaymentRequest, PaymentStatus, StagePayments,
    financial_status, newest_payment,
};

use crate::{CheckoutPorts, CommandError, Placed};

/// Asks `payment`'s provider to start collecting it, with its method's `settings`, and
/// keeps the reference the provider answered with on the payment. The next action, or
/// why there's none (`Err` inside: the provider failed, the payment stays `pending`);
/// `Err` outside only when the reference couldn't be stored.
pub(crate) async fn start<S: HasPayments, Q: PaymentProviders>(
    ports: &S,
    providers: &Q,
    order: &Order,
    payment: &mut Payment,
    settings: serde_json::Value,
    return_url: Option<&str>,
) -> Result<Result<NextAction, PaymentError>, CommandError> {
    let Some(method) = payment.method() else {
        return Ok(Err(PaymentError::UnknownProvider(
            payment.provider().clone(),
        )));
    };
    let request = PaymentRequest {
        payment: payment.id(),
        order: order.id(),
        order_number: order.number().value(),
        method,
        amount: payment.amount(),
        channel: order.channel(),
        market: order.market(),
        country: order.country(),
        customer_email: order.email().clone(),
        return_url: return_url.map(str::to_string),
        instructions: payment.instructions().map(str::to_string),
        settings,
    };
    let started = match providers.start(payment.provider(), &request).await {
        Ok(started) => started,
        Err(e) => return Ok(Err(e)),
    };
    if started.reference.is_some() {
        keep_reference(ports, payment, started.reference).await?;
    }
    Ok(Ok(started.next_action))
}

/// Stores the provider's `reference` on `payment`, reloading it when the provider's own
/// report (its events) changed it meanwhile.
async fn keep_reference<S: HasPayments>(
    ports: &S,
    payment: &mut Payment,
    reference: Option<String>,
) -> Result<(), CommandError> {
    let mut attempts = Attempts::first();
    loop {
        if !payment.set_reference(reference.clone())? {
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

/// Starts `payment` again while it's open (a replayed checkout, the shopper asking
/// again), with its method's settings: none needed for `manual`, whose instructions
/// are the payment's copy; another provider whose method is gone can't go on
/// (`payment_method_unavailable`).
pub(crate) async fn restart<S: HasPayments + HasPaymentMethods, Q: PaymentProviders>(
    ports: &S,
    providers: &Q,
    order: &Order,
    payment: &mut Payment,
    return_url: Option<&str>,
) -> Result<Result<NextAction, PaymentError>, CommandError> {
    let method = match payment.method() {
        Some(id) => ports.payment_methods().get(id).await?,
        None => None,
    };
    match (method, payment.method()) {
        (Some(method), _) => {
            let settings = method.settings().clone();
            start(ports, providers, order, payment, settings, return_url).await
        }
        _ if payment.provider().is_manual() => {
            start(
                ports,
                providers,
                order,
                payment,
                serde_json::Value::Null,
                return_url,
            )
            .await
        }
        (None, Some(id)) => Ok(Err(PaymentError::MethodUnavailable(id))),
        (None, None) => Ok(Err(PaymentError::UnknownProvider(
            payment.provider().clone(),
        ))),
    }
}

/// The shopper asking to pay for an order again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartPayment {
    pub order: OrderId,
    /// Where the storefront wants the buyer back after a redirect.
    pub return_url: Option<String>,
}

/// Starts paying for an order again (`checkout`: the storefront that placed it, holding
/// its access token, which the host checks): the provider of a `pending` payment is
/// asked again with the same idempotency key, the payment's ID; after a `failed` one, a
/// new payment of the order's total is made with the same method (still offered in the
/// order's market and channel: `payment_method_unavailable` otherwise), in one batch
/// with the order, and started. Refused for another channel's order
/// (`order_not_found`), a cancelled one (`order_cancelled`), and one with nothing to
/// start (`order_not_payable`: authorized, paid, refunded, or no payment at all).
pub async fn start_order_payment<S: CheckoutPorts, Q: PaymentProviders>(
    ctx: &Ctx,
    ports: &S,
    ids: &impl IdGenerator,
    providers: &Q,
    input: StartPayment,
) -> Result<Placed, CommandError> {
    ctx.require(Scope::Checkout)?;
    let mut attempts = Attempts::first();
    let (order, mut payment) = loop {
        match open_payment(ctx, ports, ids, providers, input.order).await {
            Err(CommandError::Repo(RepoError::StaleVersion)) if attempts.another() => continue,
            done => break done?,
        }
    };
    let next_action = restart(
        ports,
        providers,
        &order,
        &mut payment,
        input.return_url.as_deref(),
    )
    .await?;
    Ok(Placed {
        order,
        payment: Some(payment),
        next_action: Some(next_action),
    })
}

/// The order and the payment to start: its pending one, or a new one after a failed
/// one, committed with the order (whose version moves, so two requests at once can't
/// both make one: the loser reloads and finds it).
async fn open_payment<S: CheckoutPorts, Q: PaymentProviders>(
    ctx: &Ctx,
    ports: &S,
    ids: &impl IdGenerator,
    providers: &Q,
    id: OrderId,
) -> Result<(Order, Payment), CommandError> {
    let mut order = ports
        .orders()
        .get(id)
        .await?
        // Another channel's order reads as missing (design note 0020).
        .filter(|o| ctx.channel().is_none_or(|c| c == o.channel()))
        .ok_or(CommandError::OrderNotFound(id))?;
    if order.cancelled().is_some() {
        return Err(OrderError::Cancelled(id).into());
    }
    let not_payable = |order: &Order| CommandError::NotPayable {
        order: order.id(),
        status: order.financial_status(),
    };
    let newest = newest_payment(ports, id).await?;
    let failed = match newest {
        Some(p) if p.status() == PaymentStatus::Pending => return Ok((order, p)),
        Some(p) if p.status() == PaymentStatus::Failed => p,
        _ => return Err(not_payable(&order)),
    };
    if order.financial_status() != FinancialStatus::Pending {
        return Err(not_payable(&order));
    }
    let method = match failed.method() {
        Some(method) => ports.payment_methods().get(method).await?,
        None => None,
    };
    let method = match method {
        Some(m)
            if crate::payment::offered(&m, order.market(), order.channel(), providers).await? =>
        {
            m
        }
        _ => {
            return Err(match failed.method() {
                Some(m) => CommandError::PaymentMethodUnavailable(m),
                None => not_payable(&order),
            });
        }
    };
    let mut payment = Payment::new(NewPayment::by(
        &method,
        PaymentId::from_uuid(ids.new_uuid()),
        order.id(),
        order.total(),
    ))?;
    let mut payments = ports.payments().list_for_order(id).await?;
    payments.insert(0, payment.clone());
    order.set_financial_status(financial_status(order.total(), &payments)?);
    let mut batch = ports.batch();
    batch.insert_payment(&mut payment);
    batch.update_order(&mut order);
    ports.commit(batch).await?;
    Ok((order, payment))
}
