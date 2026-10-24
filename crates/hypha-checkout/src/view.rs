//! How a cart reads once checkout may have completed it.

use hypha_cart::{
    Cart, CartRepo, CartStatus, HasCarts, LineDetails, LineStatus, PricedCart, PricedLine,
    PricedShipping, PricedShippingGroup, PricingPorts, price_for_checkout,
};
use hypha_core::{CartId, Ctx, RepoError, Scope};
use hypha_orders::{HasOrders, Order, OrderRepo, OrderTax, OrderTaxSource};
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
                subtotal: line.total,
                discounts: Vec::new(),
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
                    discounts: Vec::new(),
                    total: s.amount.money(),
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
        discounts: Vec::new(),
        discount_total: hypha_core::Money::zero(order.currency()),
        discount_codes: Vec::new(),
        tax_total: order.tax_total(),
        total: order.total(),
        payment_method: cart.payment_method(),
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
