//! Starting a cart's payment before its order (design note 0031, §1.3): an online
//! method's money is committed first, and checkout places the order once it is.

use hypha_cart::{CartReading, CartRepo};
use hypha_core::{Attempts, CartId, Clock, Ctx, IdGenerator, Money, PaymentId, RepoError, Scope};
use hypha_payments::{
    NewPayment, NextAction, Payment, PaymentError, PaymentProviders, PaymentRepo, PaymentStatus,
    StagePayments,
};
use hypha_shipping::RateProvider;

use crate::commands::{Ready, ready};
use crate::start::{Payer, refresh, start};
use crate::{CheckoutPorts, CommandError};

/// The shopper starting to pay for their cart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartCartPayment {
    pub cart: CartId,
    /// The total the shopper sees: `total_changed` if the cart costs something else now.
    pub expected_total: Option<Money>,
    /// Where the storefront wants the shopper back after the provider's page.
    pub return_url: Option<String>,
}

/// The cart's payment, started, and what the shopper does next: the provider's card form
/// (`client_widget`) or page (`redirect`), or why it couldn't start (the payment stays
/// `pending`; starting again tries the same one).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CartPaymentStarted {
    pub payment: Payment,
    pub next_action: Result<NextAction, PaymentError>,
}

/// Starts paying for a cart with its online method (`checkout`; design note 0031, §3).
///
/// Runs checkout's checks first (the same errors: the cart's lines, email, shipping,
/// `total_changed`, `payment_method_required`, `payment_method_unavailable`). Refused
/// for a manual method (`payment_method_offline`: place the order), a zero total
/// (`cart_nothing_to_pay`) and a cart whose money is committed already
/// (`cart_payment_committed`: check out). A pending payment of the same method and
/// amount is started again (its provider resumes the attempt it made); otherwise a new
/// payment of the cart's total is made, any other pending ones voided in the same batch
/// and released at their provider (best effort: money one of them takes later is a
/// second collection, given back). A reused payment its provider can't go on with (an
/// attempt cancelled at the provider) makes way for a new one.
#[allow(clippy::too_many_arguments)]
pub async fn start_cart_payment<
    S: CheckoutPorts,
    I: IdGenerator,
    C: Clock,
    P: RateProvider,
    Q: PaymentProviders,
>(
    ctx: &Ctx,
    ports: &S,
    ids: &I,
    clock: &C,
    rates: &P,
    providers: &Q,
    input: StartCartPayment,
) -> Result<CartPaymentStarted, CommandError> {
    ctx.require(Scope::Checkout)?;
    let mut attempts = Attempts::first();
    let mut fresh = false;
    loop {
        let e = match start_once(ctx, ports, ids, clock, rates, providers, &input, fresh).await {
            Ok(Started::Done(started)) => return Ok(*started),
            // Only a reused payment answers this, so the next round makes a new one.
            Ok(Started::Again) => {
                fresh = true;
                continue;
            }
            Err(e) => e,
        };
        if !(matches!(e, CommandError::Repo(RepoError::StaleVersion)) && attempts.another()) {
            return Err(e);
        }
    }
}

enum Started {
    // Boxed: an answer is much larger than "again".
    Done(Box<CartPaymentStarted>),
    /// The reused payment couldn't go on: make a new one.
    Again,
}

#[allow(clippy::too_many_arguments)]
async fn start_once<
    S: CheckoutPorts,
    I: IdGenerator,
    C: Clock,
    P: RateProvider,
    Q: PaymentProviders,
>(
    ctx: &Ctx,
    ports: &S,
    ids: &I,
    clock: &C,
    rates: &P,
    providers: &Q,
    input: &StartCartPayment,
    fresh: bool,
) -> Result<Started, CommandError> {
    let mut renewed = None;
    let Ready {
        mut cart,
        reading,
        method,
    } = ready(
        ctx,
        ports,
        clock,
        rates,
        providers,
        input.cart,
        input.expected_total,
        &mut renewed,
    )
    .await?;
    if renewed.is_some() {
        // A shipping quote renewed by the checks: kept, as checkout would.
        ports.carts().update(&mut cart).await?;
    }
    let CartReading { priced, .. } = reading;
    let method = method.ok_or(CommandError::NothingToPay(cart.id()))?;
    if method.provider().is_manual() {
        return Err(CommandError::PaymentMethodOffline(method.id()));
    }
    let email = priced
        .email
        .clone()
        .ok_or(CommandError::MissingEmail(cart.id()))?;
    let existing = ports.payments().list_for_cart(cart.id()).await?;
    if let Some(done) = existing
        .iter()
        .find(|p| p.order().is_none() && p.status().committed())
    {
        return Err(CommandError::CartPaymentCommitted {
            cart: cart.id(),
            payment: done.id(),
        });
    }
    let payer = Payer {
        cart: cart.id(),
        order: None,
        channel: cart.channel(),
        market: priced.market,
        country: priced.country,
        email,
    };
    let reusable = existing
        .iter()
        .find(|p| {
            !fresh
                && p.order().is_none()
                && p.status() == PaymentStatus::Pending
                && p.method() == Some(method.id())
                && p.amount() == priced.total
        })
        .cloned();
    let reused = reusable.is_some();
    let mut payment = match reusable {
        Some(payment) => payment,
        None => {
            let mut payment = Payment::new(NewPayment::by(
                &method,
                PaymentId::from_uuid(ids.new_uuid()),
                cart.id(),
                None,
                priced.total,
            ))?;
            let now = clock.now();
            let mut stale: Vec<Payment> = existing
                .into_iter()
                .filter(|p| p.order().is_none() && p.status() == PaymentStatus::Pending)
                .collect();
            let mut batch = ports.batch();
            batch.insert_payment(&mut payment);
            for old in &mut stale {
                old.void(now)?;
                batch.update_payment(old);
            }
            ports.commit(batch).await?;
            for old in stale {
                crate::start::release_pending(providers, old).await;
            }
            payment
        }
    };
    let next_action = start(
        ports,
        providers,
        &payer,
        &mut payment,
        method.settings().clone(),
        input.return_url.as_deref(),
    )
    .await;
    if reused && matches!(next_action, Err(PaymentError::ProviderFailed { .. })) {
        // Before replacing it, where is it? Paid meanwhile (its report still on the
        // way): check out instead. Unknown (the provider is down): try it again later
        // rather than replace a payment the shopper may have paid.
        let moved = refresh(ports, providers, clock, &mut payment).await?;
        if payment.status().committed() {
            return Err(CommandError::CartPaymentCommitted {
                cart: cart.id(),
                payment: payment.id(),
            });
        }
        let gone = moved
            || matches!(
                payment.status(),
                PaymentStatus::Failed | PaymentStatus::Voided
            );
        if gone || provider_says_open(providers, &payment).await {
            return Ok(Started::Again);
        }
    }
    Ok(Started::Done(Box::new(CartPaymentStarted {
        payment,
        next_action,
    })))
}

/// Whether the provider says `payment` waits for the shopper: it answered, so its failed
/// start was about the attempt itself (cancelled there), not the provider being down.
async fn provider_says_open<Q: PaymentProviders>(providers: &Q, payment: &Payment) -> bool {
    let request = hypha_payments::StatusRequest {
        payment: payment.id(),
        cart: payment.cart(),
        reference: payment.reference().map(str::to_string),
        attempt: payment.attempt().map(str::to_string),
        store_settings: serde_json::Map::new(),
    };
    matches!(
        providers.status(payment.provider(), &request).await,
        Ok(hypha_payments::ProviderStatus {
            state: hypha_payments::ProviderState::Open | hypha_payments::ProviderState::Canceled,
            ..
        })
    )
}
