import type { Metadata } from "next";
import { redirect } from "next/navigation";

import { Totals } from "@/components/totals";
import { api, countryOf } from "@/lib/api";
import { formatMoney } from "@/lib/money";
import { cartId, link } from "@/lib/site";

import { placeOrder, saveDetails, selectShipping } from "../actions";

export const metadata: Metadata = { title: "Checkout" };

/**
 * Checkout (design note 0023, §3) on one page: email and shipping address, a shipping
 * option per group (0015), then "Place order", unpaid (0021, SU8).
 */
export default async function CheckoutPage({
  params,
  searchParams,
}: {
  params: Promise<{ country: string }>;
  searchParams: Promise<{ error?: string }>;
}) {
  const country = await countryOf((await params).country);
  const code = country.country;
  const digits = country.minor_units;
  const { error } = await searchParams;
  const id = await cartId(code);
  const client = api();
  const cart = id
    ? (await client.GET("/storefront/carts/{id}", { params: { path: { id } } })).data
    : undefined;
  if (!cart || cart.order || cart.lines.length === 0) redirect(link(code, "/cart"));

  const address = cart.shipping_address;
  const ships = cart.shipping_groups.length > 0;
  const offers =
    ships && address
      ? (
          await client.GET("/storefront/carts/{id}/shipping-options", {
            params: { path: { id: cart.id } },
          })
        ).data?.groups ?? []
      : [];
  const detailsDone = Boolean(cart.email) && (!ships || Boolean(address));
  const ready = detailsDone && cart.fully_available && (!ships || cart.shipping_ready);

  return (
    <>
      <h1>Checkout</h1>
      {error && <p className="error">{error}</p>}
      <div className="checkout">
        <div className="stack">
          <form action={saveDetails} className="panel stack">
            <h2 style={{ margin: 0 }}>1. Your details</h2>
            <input type="hidden" name="country" value={code} />
            <label>
              Email
              <input type="email" name="email" required defaultValue={cart.email ?? ""} />
            </label>
            <div className="fields">
              <label>
                First name
                <input name="first_name" defaultValue={address?.first_name ?? ""} />
              </label>
              <label>
                Last name
                <input name="last_name" required defaultValue={address?.last_name ?? ""} />
              </label>
            </div>
            <label>
              Company (optional)
              <input name="company" defaultValue={address?.company ?? ""} />
            </label>
            <label>
              Address
              <input name="line1" required defaultValue={address?.line1 ?? ""} />
            </label>
            <label>
              Apartment, suite (optional)
              <input name="line2" defaultValue={address?.line2 ?? ""} />
            </label>
            <div className="fields">
              <label>
                Postal code
                <input name="postal_code" defaultValue={address?.postal_code ?? ""} />
              </label>
              <label>
                City
                <input name="city" required defaultValue={address?.city ?? ""} />
              </label>
            </div>
            <div className="fields">
              <label>
                Region (optional)
                <input name="province" defaultValue={address?.province ?? ""} />
              </label>
              <label>
                Phone (optional)
                <input name="phone" type="tel" defaultValue={address?.phone ?? ""} />
              </label>
            </div>
            <p className="muted" style={{ margin: 0 }}>
              Ships to {country.name}. To ship elsewhere, change the country at the top.
            </p>
            <div>
              <button className="button" type="submit">
                {detailsDone ? "Update details" : "Continue"}
              </button>
            </div>
          </form>

          {ships && (
            <div className="panel stack">
              <h2 style={{ margin: 0 }}>2. Shipping</h2>
              {!address && <p className="muted">Enter your address to see the options.</p>}
              {offers.map((group, index) => {
                const chosen = cart.shipping_groups.find((g) => g.id === group.group);
                return (
                  <form key={group.group} action={selectShipping} className="stack">
                    <input type="hidden" name="country" value={code} />
                    <input type="hidden" name="group" value={group.group} />
                    {offers.length > 1 && <strong>Shipment {index + 1}</strong>}
                    {chosen?.problem && (
                      <p className="error">That option can't be used now: choose again.</p>
                    )}
                    {group.offers.length === 0 ? (
                      <p className="muted">No way to ship this here yet.</p>
                    ) : (
                      <div className="offers">
                        {group.offers.map((offer) => (
                          <label key={offer.option} className="offer">
                            <input
                              type="radio"
                              name="option"
                              value={offer.option}
                              defaultChecked={chosen?.selected?.option === offer.option}
                              required
                            />
                            <span style={{ flex: 1 }}>
                              {offer.name}
                              {offer.estimate && (
                                <span className="muted">
                                  {" "}
                                  · {offer.estimate.min_days}–{offer.estimate.max_days} days
                                </span>
                              )}
                            </span>
                            <span>{formatMoney(offer.amount, digits)}</span>
                          </label>
                        ))}
                      </div>
                    )}
                    {group.offers.length > 0 && (
                      <div>
                        <button className="button button-quiet" type="submit">
                          Use this option
                        </button>
                      </div>
                    )}
                  </form>
                );
              })}
            </div>
          )}
        </div>

        <aside className="panel stack" style={{ alignSelf: "start" }}>
          <h2 style={{ margin: 0 }}>{ships ? "3." : "2."} Order summary</h2>
          {cart.lines.map((line) => (
            <div key={line.variant} style={{ display: "flex", justifyContent: "space-between", gap: 12 }}>
              <span>
                {line.quantity} × {line.product_title}
                {line.variant_title && <span className="muted"> ({line.variant_title})</span>}
              </span>
              <span>{line.status === "priced" ? formatMoney(line.total, digits) : "—"}</span>
            </div>
          ))}
          <Totals
            subtotal={cart.subtotal}
            shipping={cart.shipping_total}
            tax={cart.tax_total}
            total={cart.total}
            taxesIncluded={cart.taxes_included}
            digits={digits}
          />
          <form action={placeOrder} className="stack">
            <input type="hidden" name="country" value={code} />
            {/* The total shown: checkout refuses one that moved (`total_changed`). */}
            <input type="hidden" name="total_amount" value={cart.total.amount} />
            <input type="hidden" name="total_currency" value={cart.total.currency} />
            {/* One per render: a double click places one order. */}
            <input type="hidden" name="attempt" value={crypto.randomUUID()} />
            <button className="button" type="submit" disabled={!ready}>
              Place order
            </button>
            <p className="muted" style={{ margin: 0, fontSize: "0.85rem" }}>
              You pay on delivery: no payment is taken online.
            </p>
          </form>
        </aside>
      </div>
    </>
  );
}
