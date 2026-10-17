import { type Money, formatMoney } from "@/lib/money";

/** A cart's or an order's totals, with tax as the market shows it. */
export function Totals({
  subtotal,
  shipping,
  tax,
  total,
  taxesIncluded,
  digits,
}: {
  subtotal: Money;
  shipping: Money;
  tax: Money;
  total: Money;
  taxesIncluded: boolean;
  digits: number;
}) {
  return (
    <div className="totals">
      <div>
        <span>Subtotal</span>
        <span>{formatMoney(subtotal, digits)}</span>
      </div>
      <div>
        <span>Shipping</span>
        <span>{formatMoney(shipping, digits)}</span>
      </div>
      <div className="muted">
        <span>{taxesIncluded ? "Including tax" : "Tax"}</span>
        <span>{formatMoney(tax, digits)}</span>
      </div>
      <div className="total-row">
        <span>Total</span>
        <span>{formatMoney(total, digits)}</span>
      </div>
    </div>
  );
}
