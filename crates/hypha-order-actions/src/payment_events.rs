//! What a payment's provider reports (design note 0029, §3.1): an app learns that a
//! card was authorized, captured, declined, released or refunded (from its own
//! provider's webhooks, Stripe's), and tells Hypha through
//! `POST /admin/payments/{id}/events`. Each report moves the payment and its order's
//! `financial_status` in one batch, under 0027's rules (the moves `PaymentStatus`
//! allows). A payment started for a cart before its order (design note 0031) has no
//! order yet: the report moves the payment alone, and the host completes the checkout
//! once it says the money is committed (`hypha_checkout::complete_paid_checkout`).

use hypha_apps::{AppRepo, HasApps};
use hypha_core::{
    Actor, Attempts, Clock, Ctx, HasIdempotency, IdempotencyRecord, IdempotencyRequest, Idempotent,
    Money, PaymentId, RepoError, Scope, StageIdempotency, Timestamp, earlier_id, optional_text,
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
    /// Taken and being collected (SEPA Direct Debit: days; design note 0031, Q3).
    Processing,
    Paid,
    Failed,
    Voided,
    /// Some of it was given back (`amount`).
    PartiallyRefunded,
    /// All that was left was given back.
    Refunded,
}

/// What a report left: the payment, and its order with all its payments once it has one
/// (`None` for a cart's payment before checkout placed the order: design note 0031).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaymentReport {
    pub payment: Payment,
    pub order: Option<OrderPayments>,
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
    /// The provider's ID for the attempt this report is about (a Stripe PaymentIntent;
    /// for a refund, the attempt it refunds), from a provider whose IDs name attempts:
    /// the Stripe spore, or an app through the events route. A paid or authorized
    /// report that moves the payment, or names its own reference, keeps it as the
    /// payment's `attempt`; a report naming another is refused (`other_attempt`).
    /// Absent, nothing is checked (a provider whose capture or void carries an ID of
    /// its own leaves it out), and neither is a payment without an attempt yet.
    /// Up to 255 characters.
    pub attempt: Option<String>,
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
/// the status doesn't allow (`invalid_payment_transition`), a report about another
/// attempt than the payment's when it names one (`payment_reference_mismatch`,
/// `other_attempt`), an amount that doesn't fit
/// (`invalid_refund_amount`, `payment_amount_mismatch`). A cancelled order still takes
/// reports: they say where the money is.
pub async fn record_payment_event<S: OrderActionPorts + HasApps>(
    ctx: &Ctx,
    ports: &S,
    clock: &impl Clock,
    input: PaymentEvent,
) -> Result<Idempotent<PaymentReport>, CommandError> {
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
) -> Result<PaymentReport, CommandError> {
    let mut payment = ports
        .payments()
        .get(input.payment)
        .await?
        .ok_or(CommandError::PaymentNotFound(input.payment))?;
    check_reporter(ctx, ports, &payment).await?;
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
    let idempotency = input
        .idempotency
        .as_ref()
        .map(|request| {
            IdempotencyRecord::new(
                ctx.actor(),
                request,
                PAYMENT_EVENT,
                payment.id().as_uuid(),
                clock.now(),
            )
        })
        .transpose()?;

    // A cart's payment before its order: the payment alone.
    let Some(order_id) = payment.order() else {
        if !apply(&mut payment, input, when)? {
            return Ok(PaymentReport {
                payment,
                order: None,
            });
        }
        let mut batch = ports.batch();
        batch.update_payment(&mut payment);
        if let Some(idempotency) = idempotency {
            batch.record_idempotency(idempotency);
        }
        ports.commit(batch).await?;
        return Ok(PaymentReport {
            payment,
            order: None,
        });
    };

    let mut order = load_order(ports, order_id).await?;
    let mut payments = ports.payments().list_for_order(order.id()).await?;
    let Some(at) = payments.iter().position(|p| p.id() == payment.id()) else {
        return Err(RepoError::Backend(format!(
            "payment {} isn't among its order's payments",
            payment.id()
        ))
        .into());
    };
    if !apply(&mut payments[at], input, when)? {
        return Ok(PaymentReport {
            payment: payments[at].clone(),
            order: Some(OrderPayments { order, payments }),
        });
    }
    order.set_financial_status(financial_status(order.total(), &payments)?);

    let mut batch = ports.batch();
    batch.update_payment(&mut payments[at]);
    // Always, so two reports on one order's payments can't both commit on stale
    // views of the other: the loser reloads.
    batch.update_order(&mut order);
    if let Some(idempotency) = idempotency {
        batch.record_idempotency(idempotency);
    }
    ports.commit(batch).await?;
    Ok(PaymentReport {
        payment: payments[at].clone(),
        order: Some(OrderPayments { order, payments }),
    })
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
    let attempt = other_attempt(payment, input)?;
    Ok(match input.kind {
        PaymentEventKind::Authorized => {
            whole(payment)?;
            collect(payment, attempt, |p| p.authorize(reference))?
        }
        PaymentEventKind::Paid => {
            whole(payment)?;
            collect(payment, attempt, |p| p.pay(at, reference))?
        }
        PaymentEventKind::Processing => {
            whole(payment)?;
            collect(payment, attempt, |p| p.process(reference))?
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

/// Refuses a report naming another attempt than the one the payment collects through
/// (`payment_reference_mismatch`; the payment's `attempt`, set by its start for a
/// provider whose IDs name attempts, or by a report naming one): a second attempt's
/// money for a payment past pending (paid in an old tab: the payment collects once, and
/// its provider gives that money back), or another attempt's decline, cancel or refund.
/// A pending payment may be paid or authorized through another attempt (it was started
/// again). A payment with no attempt (its provider's IDs don't name attempts, its
/// reports named none so far, or it's from before they were kept) isn't checked. In
/// the transaction that writes, so two attempts' reports racing can't both pass.
/// Returns the report's attempt, checked.
fn other_attempt(payment: &Payment, input: &PaymentEvent) -> Result<Option<String>, PaymentError> {
    let attempt = optional_text("attempt", input.attempt.clone())?;
    let (Some(named), Some(held)) = (&attempt, payment.attempt()) else {
        return Ok(attempt);
    };
    let restarted = payment.status() == PaymentStatus::Pending
        && matches!(
            input.kind,
            PaymentEventKind::Authorized | PaymentEventKind::Paid | PaymentEventKind::Processing
        );
    if held == named || restarted {
        return Ok(attempt);
    }
    Err(PaymentError::ReferenceMismatch {
        payment: payment.id(),
        status: payment.status(),
        held: held.to_string(),
        reported: named.clone(),
    })
}

/// Whether a collecting report that changes nothing names the payment's own attempt: its
/// reference (Stripe delivering the intent's report again), so a payment paid before
/// attempts were kept takes it. A collecting report that moves the payment (from
/// pending, or a capture of its authorization) is about its own collection anyway;
/// one that changes nothing under another ID may be a second attempt's money, whose
/// attempt isn't taken.
fn names_own(payment: &Payment, attempt: Option<&str>) -> bool {
    attempt.is_some() && payment.reference() == attempt
}

/// Moves `payment` with `report` (an authorization or a payment), keeping the report's
/// `attempt` as the payment's when the report moved it or names its own reference
/// (`names_own`): the one rule for both. Whether that changed anything.
fn collect(
    payment: &mut Payment,
    attempt: Option<String>,
    report: impl FnOnce(&mut Payment) -> Result<bool, PaymentError>,
) -> Result<bool, PaymentError> {
    let own = names_own(payment, attempt.as_deref());
    let changed = report(payment)?;
    let noted = if changed || own {
        payment.note_attempt(attempt)?
    } else {
        false
    };
    Ok(changed || noted)
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

/// A payment, and its order and the order's payments, as they are now.
async fn payments_of<S: OrderActionPorts>(
    ports: &S,
    id: PaymentId,
) -> Result<PaymentReport, CommandError> {
    let payment = ports
        .payments()
        .get(id)
        .await?
        .ok_or(CommandError::PaymentNotFound(id))?;
    let order = match payment.order() {
        Some(order) => {
            let order = load_order(ports, order).await?;
            let payments = ports.payments().list_for_order(order.id()).await?;
            Some(OrderPayments { order, payments })
        }
        None => None,
    };
    Ok(PaymentReport { payment, order })
}
