import type { Metadata } from "next";

import { Totals } from "@/components/totals";
import { api, countryOf } from "@/lib/api";
import { formatMoney } from "@/lib/money";
import { cartId, link } from "@/lib/site";

import { setQuantity } from "../actions";

export const metadata: Metadata = { title: "Cart" };

/**
 * The cart (design note 0023, §3): lines with quantities, unavailable ones saying why
 * (they're left out of the totals), then checkout.
 */
export default async function CartPage({
  params,
  searchParams,
}: {
  params: Promise<{ country: string }>;
  searchParams: Promise<{ error?: string }>;
}) {
  const country = await countryOf((await params).country);
  const code = country.country;
  const { error } = await searchParams;
  const id = await cartId(code);
  const cart = id
    ? (await api().GET("/storefront/carts/{id}", { params: { path: { id } } })).data
    : undefined;
  const open = cart && !cart.order ? cart : undefined;
  const digits = country.minor_units;

  return (
    <>
      <h1>Cart</h1>
      {error && <p className="error">{error}</p>}
      {!open || open.lines.length === 0 ? (
        <p className="muted">
          Your cart is empty. <a href={link(code, "/")}>Keep shopping</a>
        </p>
      ) : (
        <>
          <table className="lines">
            <thead>
              <tr>
                <th>Product</th>
                <th>Quantity</th>
                <th className="num">Total</th>
              </tr>
            </thead>
            <tbody>
              {open.lines.map((line) => (
                <tr key={line.variant}>
                  <td>
                    <strong>{line.product_title ?? "A product no longer sold"}</strong>
                    {line.variant_title && <div className="muted">{line.variant_title}</div>}
                    {line.status === "unavailable" && (
                      <div className="error" style={{ marginTop: 6, padding: "4px 8px" }}>
                        {line.reason}
                      </div>
                    )}
                  </td>
                  <td>
                    <form action={setQuantity} style={{ display: "flex", gap: 8 }}>
                      <input type="hidden" name="country" value={code} />
                      <input type="hidden" name="variant" value={line.variant} />
                      <input
                        type="number"
                        name="quantity"
                        min={0}
                        defaultValue={line.quantity}
                        aria-label="Quantity"
                        style={{ width: 72 }}
                      />
                      <button className="link-button" type="submit">
                        Update
                      </button>
                    </form>
                    <form action={setQuantity}>
                      <input type="hidden" name="country" value={code} />
                      <input type="hidden" name="variant" value={line.variant} />
                      <input type="hidden" name="quantity" value="0" />
                      <button className="link-button" type="submit">
                        Remove
                      </button>
                    </form>
                  </td>
                  <td className="num">
                    {line.status === "priced" ? formatMoney(line.total, digits) : "—"}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          <Totals
            subtotal={open.subtotal}
            shipping={open.shipping_total}
            tax={open.tax_total}
            total={open.total}
            taxesIncluded={open.taxes_included}
            digits={digits}
          />
          <p style={{ textAlign: "right", marginTop: 24 }}>
            {open.fully_available ? (
              <a className="button" href={link(code, "/checkout")}>
                Check out
              </a>
            ) : (
              <span className="muted">Remove what can't be bought to check out.</span>
            )}
          </p>
        </>
      )}
    </>
  );
}
