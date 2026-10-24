//! How a cart reads once checkout may have completed it.

use hypha_cart::{
    Cart, CartRepo, CartStatus, HasCarts, LineDetails, LineStatus, PricedCart, PricedLine,
    PricedShipping, PricedShippingGroup, PricingPorts,
    discounts::{
        Application, Class, Share, Target, Value,
        json::{target_from_json, value_from_json},
    },
    price_for_checkout,
};
use hypha_core::BasisPoints;
use hypha_core::{CartId, Ctx, RepoError, Scope};
use hypha_orders::{
    DiscountClass, DiscountShare, HasOrders, Order, OrderDiscount, OrderRepo, OrderTax,
    OrderTaxSource,
};
use hypha_shipping::DeliveryEstimate;
use hypha_tax::{TaxLine, TaxSource};

use crate::CommandError;

/// The cart as a buyer sees it. An open cart is priced now, like `hypha_cart::get_cart`.
/// A checked-out cart shows what its order charged: repricing it would use today's
/// prices and stock, and the order's own reservation would make its lines look out of
/// stock.
///
/// Here rather than in `hypha-cart` because it reads orders, and the cart crate doesn't
/// depend on them.
pub async fn view_cart<S: HasCarts + HasOrders + PricingPorts>(
    ctx: &Ctx,
    ports: &S,
    id: CartId,
) -> Result<PricedCart, CommandError> {
    ctx.require(Scope::CartsRead)?;
    let cart = ports
        .carts()
        .get(id)
        .await?
        // Another channel's cart reads as missing.
        .filter(|cart| cart.visible_in(ctx.channel()))
        .ok_or(CommandError::CartNotFound(id))?;
    let CartStatus::Completed { order } = cart.status() else {
        let (priced, _) = price_for_checkout(ports, &cart).await?;
        return Ok(priced);
    };
    // The order commits in the same batch that completes the cart, so it exists.
    let order = ports.orders().get(order).await?.ok_or_else(|| {
        RepoError::Backend(format!(
            "cart {id} names order {order}, which doesn't exist"
        ))
    })?;
    Ok(as_charged(&cart, &order))
}

/// `cart` with its order's lines and totals. Email and customer stay the cart's own.
fn as_charged(cart: &Cart, order: &Order) -> PricedCart {
    let lines = order
        .lines()
        .iter()
        .map(|line| PricedLine {
            product: line.product,
            variant: line.variant,
            quantity: line.quantity,
            details: Some(LineDetails {
                product_title: line.product_title.clone(),
                variant_title: line.variant_title.clone(),
                sku: line.sku.clone(),
                handle: None,
                image: None,
            }),
            // Checked out: nothing more to buy on this line.
            sellable: Some(0),
            status: LineStatus::Priced {
                unit_price: line.unit_price,
                compare_at: line.compare_at_price,
                price_list: line.price_list,
                subtotal: line.subtotal,
                discounts: shares(&line.discounts),
                total: line.total,
                tax: line.tax.map(tax_line),
            },
        })
        .collect();
    PricedCart {
        id: cart.id(),
        market: order.market(),
        currency: order.currency(),
        country: order.country(),
        email: cart.email().cloned(),
        customer: cart.customer(),
        order: Some(order.id()),
        marketing_opt_in: cart.marketing_opt_in(),
        lines,
        taxes_included: order.taxes_included(),
        subtotal: order.subtotal(),
        shipping_address: order.shipping_address().cloned(),
        shipping_groups: order
            .shipping_lines()
            .iter()
            .map(|s| PricedShippingGroup {
                id: s.group,
                lines: order
                    .lines()
                    .iter()
                    .filter(|l| s.lines.contains(&l.id))
                    .map(|l| l.variant)
                    .collect(),
                selected: Some(PricedShipping {
                    option: s.option,
                    code: s.code.clone(),
                    name: s.name.clone(),
                    amount: s.amount.money(),
                    discounts: shares(&s.discounts),
                    total: s.total,
                    tax: s.tax.map(tax_line),
                    estimate: s
                        .estimate_days
                        .and_then(|(min, max)| DeliveryEstimate::new(min, max).ok()),
                    // The order keeps the quote's reference, not the quote itself.
                    quote: None,
                }),
                problem: None,
            })
            .collect(),
        shipping_total: order.shipping_total(),
        discounts: order.discounts().iter().map(application).collect(),
        discount_total: order.discount_total(),
        // Checked out: what applied is in `discounts`; the codes' states were for pricing.
        discount_codes: Vec::new(),
        tax_total: order.tax_total(),
        total: order.total(),
        payment_method: cart.payment_method(),
    }
}

fn shares(shares: &[DiscountShare]) -> Vec<Share> {
    shares
        .iter()
        .map(|s| Share {
            application: usize::from(s.application),
            amount: s.amount,
            units: s.units,
        })
        .collect()
}

/// An order's discount as the cart showed it. Its value and target are the discounts
/// domain's JSON, written by checkout; JSON in another shape reads as nothing off the
/// subtotal (it can't be, short of a bug), so the cart still reads.
fn application(d: &OrderDiscount) -> Application {
    Application {
        discount: d.discount,
        code: d.code.clone(),
        title: d.title.clone(),
        summary: d.summary.clone(),
        class: match d.class {
            DiscountClass::Product => Class::Product,
            DiscountClass::Order => Class::Order,
            DiscountClass::Shipping => Class::Shipping,
        },
        value: value_from_json(&d.value).unwrap_or(Value::Percent(BasisPoints::ZERO)),
        target: target_from_json(&d.target).unwrap_or(Target::Subtotal),
        amount: d.amount,
    }
}

fn tax_line(tax: OrderTax) -> TaxLine {
    TaxLine {
        rate: tax.rate,
        class: tax.class,
        amount: tax.amount,
        source: match tax.source {
            OrderTaxSource::RateTable { region } => TaxSource::RateTable { region },
        },
    }
}
