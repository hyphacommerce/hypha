import type { Metadata } from "next";
import { cookies } from "next/headers";

import { Totals } from "@/components/totals";
import { api, countryOf } from "@/lib/api";
import { formatMoney } from "@/lib/money";
import { link } from "@/lib/site";

export const metadata: Metadata = { title: "Thank you" };

/**
 * The order's confirmation (design note 0023, §3), read through the checked-out cart:
 * it names its order, and its lines and totals are what was bought.
 */
export default async function OrderPage({ params }: { params: Promise<{ country: string }> }) {
  const country = await countryOf((await params).country);
  const code = country.country;
  const digits = country.minor_units;
  const id = (await cookies()).get(`order_${code.toLowerCase()}`)?.value;
  const cart = id
    ? (await api().GET("/storefront/carts/{id}", { params: { path: { id } } })).data
    : undefined;

  if (!cart?.order) {
    return (
      <>
        <h1>No recent order</h1>
        <p className="muted">
          <a href={link(code, "/")}>Back to the shop</a>
        </p>
      </>
    );
  }

  return (
    <div className="stack" style={{ maxWidth: 640 }}>
      <h1>Thank you</h1>
      <p>
        Your order is placed. We'll write to <strong>{cart.email}</strong> about it.
      </p>
      <div className="panel stack">
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
      </div>
      {cart.shipping_address && (
        <p className="muted">
          Ships to {cart.shipping_address.first_name} {cart.shipping_address.last_name},{" "}
          {cart.shipping_address.line1}, {cart.shipping_address.city}.
        </p>
      )}
      <p>
        <a className="button button-quiet" href={link(code, "/")}>
          Keep shopping
        </a>
      </p>
    </div>
  );
}
