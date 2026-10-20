//! What a payment's provider reports (design note 0029, §3.1): an app learns that a
//! card was authorized, captured, declined, released or refunded (from its own
//! provider's webhooks, Stripe's), and tells Hypha through
//! `POST /admin/payments/{id}/events`. Each report moves the payment and its order's
//! `financial_status` in one batch, under 0027's rules (the moves `PaymentStatus`
//! allows).

use hypha_apps::{AppRepo, HasApps};
use hypha_core::{
    Actor, Attempts, Clock, Ctx, HasIdempotency, IdempotencyRecord, IdempotencyRequest, Idempotent,
    Money, PaymentId, RepoError, Scope, StageIdempotency, Timestamp, earlier_id,
};
use hypha_orders::StageOrders;
use hypha_payments::{
    Payment, PaymentError, PaymentRepo, PaymentStatus, StagePayments, financial_status,
};

use crate::{CommandError, OrderActionPorts, OrderPayments, commands::load_order};

/// The operation name a reported event's idempotency records carry.
pub const PAYMENT_EVENT: &str = "payment_event";

/// What happened to a payment, as its provider reports it: the status it's in now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PaymentEventKind {
    Authorized,
    Paid,
    Failed,
    Voided,
    /// Some of it was given back (`amount`).
    PartiallyRefunded,
    /// All that was left was given back.
    Refunded,
}

/// A provider's report on a payment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaymentEvent {
    pub payment: PaymentId,
    pub kind: PaymentEventKind,
    /// In the payment's minor units. For a refund, how much this one gave back
    /// (required for `partially_refunded`; for `refunded`, absent is all that was
    /// left); for any other kind, absent or the payment's whole amount (no partial
    /// captures).
    pub amount: Option<i64>,
    /// The provider's ID for the payment, kept on it; for a refund (`partially_refunded`,
    /// `refunded`), the provider's ID for that refund, kept in the payment's
    /// `refund_references` and not as its reference. A refund reported again under an
    /// ID counted already changes nothing (a provider's webhook delivered twice, or
    /// reported under another idempotency key). An app must send it for refunds
    /// (`refund_reference_required`).
    pub reference: Option<String>,
    /// When it happened at the provider; absent is now. When it was paid or voided.
    pub occurred_at: Option<Timestamp>,
    /// A retry with the same key changes nothing again and answers as the first did.
    pub idempotency: Option<IdempotencyRequest>,
}

/// Whether `ctx` may report on `payment`. An app calling with its own OAuth token
/// (`Actor::App`, design note 0029, §15), or a spore acting as its app (design note
/// 0030, §3), only on the payments it collects: their provider belongs to it
/// (`ProviderKey::belongs_to`). Staff, API keys and the system may report on any payment
/// with `payments:write`.
async fn check_reporter<S: HasApps>(
    ctx: &Ctx,
    ports: &S,
    payment: &Payment,
) -> Result<(), CommandError> {
    let Actor::App(caller) = ctx.actor() else {
        return Ok(());
    };
    // An app collects under its key, a spore under its key or its key and `_…`.
    let caller_app = ports.apps().get_by_id(caller).await?;
    if caller_app.is_some_and(|app| payment.provider().belongs_to(app.key())) {
        return Ok(());
    }
    Err(CommandError::PaymentNotYours {
        payment: payment.id(),
        app: caller,
    })
}

/// Records what a payment's provider reports (`payments:write`), moving the payment
/// (`payment.authorized`, `.paid`, `.failed`, `.voided`, `.refunded`) and its order's
/// payment status (`order.paid` once paid) in one batch with the idempotency key,
/// retried when another request changed them first. A refund reported again under the
/// provider's ID for it (`reference`) changes nothing, and an app must send that ID
/// (`refund_reference_required`). A report of the status the payment is in already
/// changes nothing (a webhook delivered twice). Refused: an unknown payment
/// (`payment_not_found`), another app's payment (`payment_not_yours`), a move
/// the status doesn't allow (`invalid_payment_transition`), a report under another
/// reference than the one the payment holds money through (`payment_reference_mismatch`,
/// `held_elsewhere`), an amount that doesn't fit
/// (`invalid_refund_amount`, `payment_amount_mismatch`). A cancelled order still takes
/// reports: they say where the money is.
pub async fn record_payment_event<S: OrderActionPorts + HasApps>(
    ctx: &Ctx,
    ports: &S,
    clock: &impl Clock,
    input: PaymentEvent,
) -> Result<Idempotent<OrderPayments>, CommandError> {
    ctx.require(Scope::PaymentsWrite)?;
    if let Some(earlier) = replay_of(ctx, ports, input.idempotency.as_ref()).await? {
        return Ok(Idempotent::replayed(payments_of(ports, earlier).await?));
    }
    let mut attempts = Attempts::first();
    loop {
        let e = match record_once(ctx, ports, clock, &input).await {
            Err(e) => e,
            Ok(done) => return Ok(Idempotent::fresh(done)),
        };
        if let Some(earlier) = replay_of(ctx, ports, input.idempotency.as_ref()).await? {
            return Ok(Idempotent::replayed(payments_of(ports, earlier).await?));
        }
        if !(matches!(e, CommandError::Repo(RepoError::StaleVersion)) && attempts.another()) {
            return Err(e);
        }
    }
}

async fn record_once<S: OrderActionPorts + HasApps>(
    ctx: &Ctx,
    ports: &S,
    clock: &impl Clock,
    input: &PaymentEvent,
) -> Result<OrderPayments, CommandError> {
    let payment = ports
        .payments()
        .get(input.payment)
        .await?
        .ok_or(CommandError::PaymentNotFound(input.payment))?;
    check_reporter(ctx, ports, &payment).await?;
    let mut order = load_order(ports, payment.order()).await?;
    let mut payments = ports.payments().list_for_order(order.id()).await?;
    let Some(at) = payments.iter().position(|p| p.id() == payment.id()) else {
        return Err(RepoError::Backend(format!(
            "payment {} isn't among its order's payments",
            payment.id()
        ))
        .into());
    };
    let refunding = matches!(
        input.kind,
        PaymentEventKind::PartiallyRefunded | PaymentEventKind::Refunded
    );
    let blank = input
        .reference
        .as_deref()
        .is_none_or(|r| r.trim().is_empty());
    if refunding && blank && matches!(ctx.actor(), Actor::App(_)) {
        return Err(CommandError::RefundReferenceRequired(payment.id()));
    }
    let when = input.occurred_at.unwrap_or_else(|| clock.now());
    if !apply(&mut payments[at], input, when)? {
        return Ok(OrderPayments { order, payments });
    }
    order.set_financial_status(financial_status(order.total(), &payments)?);

    let mut batch = ports.batch();
    batch.update_payment(&mut payments[at]);
    // Always, so two reports on one order's payments can't both commit on stale
    // views of the other: the loser reloads.
    batch.update_order(&mut order);
    if let Some(request) = &input.idempotency {
        let record = IdempotencyRecord::new(
            ctx.actor(),
            request,
            PAYMENT_EVENT,
            payment.id().as_uuid(),
            clock.now(),
        )?;
        batch.record_idempotency(record);
    }
    ports.commit(batch).await?;
    Ok(OrderPayments { order, payments })
}

/// Moves `payment` as `input` reports; whether that changed anything.
fn apply(payment: &mut Payment, input: &PaymentEvent, at: Timestamp) -> Result<bool, CommandError> {
    let currency = payment.amount().currency();
    let reference = input.reference.clone();
    let whole = |payment: &Payment| match input.amount {
        Some(amount) if amount != payment.amount().amount() => Err(CommandError::AmountMismatch {
            payment: payment.id(),
            expected: payment.amount().amount(),
            reported: amount,
        }),
        _ => Ok(()),
    };
    let refund = |payment: &mut Payment, status: PaymentStatus| {
        // A refund counted already under its reference is nothing again
        // (`Payment::refund`); without one, a payment refunded wholly takes a repeated
        // `refunded` as nothing.
        if payment.status() == status && status == PaymentStatus::Refunded {
            return Ok::<bool, CommandError>(false);
        }
        let amount = input
            .amount
            .map(|a| Money::new(a, currency))
            .transpose()
            .map_err(PaymentError::from)?;
        Ok(payment.refund(amount, status, reference.clone())?)
    };
    held_elsewhere(payment, input)?;
    Ok(match input.kind {
        PaymentEventKind::Authorized => {
            whole(payment)?;
            payment.authorize(reference)?
        }
        PaymentEventKind::Paid => {
            whole(payment)?;
            payment.pay(at, reference)?
        }
        PaymentEventKind::Failed => payment.fail(reference)?,
        PaymentEventKind::Voided => {
            let changed = payment.void(at)?;
            if changed {
                payment.set_reference(reference)?;
            }
            changed
        }
        PaymentEventKind::PartiallyRefunded => {
            if input.amount.is_none() {
                return Err(PaymentError::InvalidRefund {
                    payment: payment.id(),
                    amount: 0,
                    paid: payment.amount().amount(),
                    refunded: payment.refunded().amount(),
                    reported: PaymentStatus::PartiallyRefunded,
                }
                .into());
            }
            refund(payment, PaymentStatus::PartiallyRefunded)?
        }
        PaymentEventKind::Refunded => refund(payment, PaymentStatus::Refunded)?,
    })
}

/// Refuses a report about another attempt than the one a payment holds money through
/// (`payment_reference_mismatch`): authorized, paid or refunded under one reference, it
/// can't be authorized, paid, failed or voided under another (a second attempt at the
/// provider, paid in an old tab, collects once; its provider gives that money back).
/// Checked in the same transaction as the write, so two reports racing can't both
/// pass. A pending payment takes any reference (its attempt was made again), and a
/// refund names the refund, not the attempt.
fn held_elsewhere(payment: &Payment, input: &PaymentEvent) -> Result<(), PaymentError> {
    let refund = matches!(
        input.kind,
        PaymentEventKind::PartiallyRefunded | PaymentEventKind::Refunded
    );
    let holds = matches!(
        payment.status(),
        PaymentStatus::Authorized
            | PaymentStatus::Paid
            | PaymentStatus::PartiallyRefunded
            | PaymentStatus::Refunded
    );
    let reported = input
        .reference
        .as_deref()
        .map(str::trim)
        .filter(|r| !r.is_empty());
    match (refund || !holds, payment.reference(), reported) {
        (false, Some(held), Some(reported)) if held != reported => {
            Err(PaymentError::ReferenceMismatch {
                payment: payment.id(),
                status: payment.status(),
                held: held.to_string(),
                reported: reported.to_string(),
            })
        }
        _ => Ok(()),
    }
}

/// The payment an earlier request with this key reported on, if this caller sent it.
async fn replay_of<S: HasIdempotency>(
    ctx: &Ctx,
    ports: &S,
    request: Option<&IdempotencyRequest>,
) -> Result<Option<PaymentId>, CommandError> {
    earlier_id(
        ports.idempotency(),
        ctx.actor(),
        PAYMENT_EVENT,
        request,
        PaymentId::from_uuid,
    )
    .await
}

/// A payment's order and its payments, as they are now.
async fn payments_of<S: OrderActionPorts>(
    ports: &S,
    id: PaymentId,
) -> Result<OrderPayments, CommandError> {
    let payment = ports
        .payments()
        .get(id)
        .await?
        .ok_or(CommandError::PaymentNotFound(id))?;
    let order = load_order(ports, payment.order()).await?;
    let payments = ports.payments().list_for_order(order.id()).await?;
    Ok(OrderPayments { order, payments })
}
