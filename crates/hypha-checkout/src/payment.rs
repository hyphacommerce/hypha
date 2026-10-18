//! Choosing how to pay (design note 0027, §4): the methods a cart is offered, the
//! buyer's choice, and the check checkout repeats.
//!
//! Here rather than in `hypha-cart` because it reads payment methods, which the cart
//! crate doesn't depend on: the cart stores only the method's ID.

use hypha_cart::{CartPorts, CartRepo, HasCarts, PricedCart, price_for_checkout};
use hypha_core::{CartId, Ctx, PaymentMethodId, Scope};
use hypha_payments::{HasPaymentMethods, PaymentMethod, PaymentMethodRepo, PaymentProviders};

use crate::CommandError;

/// Offered in `market` and `channel`: enabled, in them (`PaymentMethod::offered_in`),
/// and of a provider the host offers (one this build has, or an installed, enabled app
/// implementing `payments.start`).
pub(crate) async fn offered<P: PaymentProviders>(
    method: &PaymentMethod,
    market: hypha_core::MarketId,
    channel: hypha_core::ChannelId,
    providers: &P,
) -> Result<bool, CommandError> {
    Ok(method.offered_in(market, channel) && providers.available(method.provider()).await?)
}

/// The methods a buyer can choose for the cart, in position order (`carts:read`).
pub async fn cart_payment_methods<S: HasCarts + HasPaymentMethods, P: PaymentProviders>(
    ctx: &Ctx,
    ports: &S,
    providers: &P,
    id: CartId,
) -> Result<Vec<PaymentMethod>, CommandError> {
    ctx.require(Scope::CartsRead)?;
    let cart = ports
        .carts()
        .get(id)
        .await?
        // Another channel's cart reads as missing (design note 0020).
        .filter(|cart| cart.visible_in(ctx.channel()))
        .ok_or(CommandError::CartNotFound(id))?;
    let mut methods = Vec::new();
    for method in ports.payment_methods().list().await? {
        if offered(&method, cart.market(), cart.channel(), providers).await? {
            methods.push(method);
        }
    }
    Ok(methods)
}

/// Sets how the buyer will pay (`carts:write`), or clears it with `None`: a method that
/// exists (`payment_method_not_found`) and is offered to the cart
/// (`payment_method_unavailable`). Checkout checks it again. Answers with the cart,
/// priced.
pub async fn set_payment_method<S: CartPorts + HasPaymentMethods, P: PaymentProviders>(
    ctx: &Ctx,
    ports: &S,
    providers: &P,
    id: CartId,
    method: Option<PaymentMethodId>,
) -> Result<PricedCart, CommandError> {
    ctx.require(Scope::CartsWrite)?;
    let mut cart = ports
        .carts()
        .get(id)
        .await?
        .filter(|cart| cart.visible_in(ctx.channel()))
        .ok_or(CommandError::CartNotFound(id))?;
    if let hypha_cart::CartStatus::Completed { order } = cart.status() {
        return Err(CommandError::CartCompleted { cart: id, order });
    }
    if let Some(method) = method {
        let found = ports
            .payment_methods()
            .get(method)
            .await?
            .ok_or(CommandError::PaymentMethodNotFound(method))?;
        if !offered(&found, cart.market(), cart.channel(), providers).await? {
            return Err(CommandError::PaymentMethodUnavailable(method));
        }
    }
    if cart.payment_method() != method {
        cart.set_payment_method(method)
            .map_err(hypha_cart::CommandError::from)?;
        ports.carts().update(&mut cart).await?;
    }
    let (priced, _) = price_for_checkout(ports, &cart).await?;
    Ok(priced)
}

/// The method checkout charges with: the cart's choice, still offered
/// (`payment_method_unavailable` otherwise: gone, disabled, moved out of the market).
/// A cart that chose none is refused (`payment_method_required`) unless there's nothing
/// to pay (design note 0027, Q2); then no payment is made, whatever was chosen.
pub(crate) async fn checkout_method<S: HasPaymentMethods, P: PaymentProviders>(
    ports: &S,
    providers: &P,
    cart: &hypha_cart::Cart,
    priced: &PricedCart,
) -> Result<Option<PaymentMethod>, CommandError> {
    if priced.total.amount() == 0 {
        return Ok(None);
    }
    let Some(id) = cart.payment_method() else {
        return Err(CommandError::PaymentMethodRequired(cart.id()));
    };
    match ports.payment_methods().get(id).await? {
        Some(method) if offered(&method, cart.market(), cart.channel(), providers).await? => {
            Ok(Some(method))
        }
        _ => Err(CommandError::PaymentMethodUnavailable(id)),
    }
}
