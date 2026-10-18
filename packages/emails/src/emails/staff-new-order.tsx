import { Text } from "react-email";
import type { Email } from "../email.ts";
import { If, v } from "../jinja.tsx";
import { Delivery, OrderLines, PrimaryButton, Totals } from "../parts.tsx";
import { heading, paragraph, subheading } from "../styles.ts";
import * as text from "../text.ts";

export const staffNewOrder: Email = {
  key: "staff_new_order",
  subject: "New order {{ order.number }}: {{ order.total.formatted }}",
  preheader:
    "{{ order.number }} for {{ order.total.formatted }}, placed {{ order.placed_at.formatted }}.",
  Content: () => (
    <>
      <Text style={heading}>New order {v("order.number")}</Text>
      <Text style={paragraph}>
        <If test="customer and customer.name" inline otherwise={v("order.email")}>
          {v("customer.name")} ({v("order.email")})
        </If>{" "}
        placed an order for {v("order.total.formatted")} on {v("order.placed_at.formatted")}.
        <If test="order.payment" inline>
          {" "}
          Payment: {v("order.payment.method")} ({v("order.payment.status")}).
        </If>
      </Text>
      <If test="admin_url">
        <PrimaryButton href={v("admin_url")}>View the order</PrimaryButton>
      </If>
      <Text style={subheading}>Items</Text>
      <OrderLines />
      <Totals />
      <Delivery />
    </>
  ),
  text: `New order {{ order.number }}

{% if customer and customer.name %}{{ customer.name }} ({{ order.email }}){% else %}{{ order.email }}{% endif %} placed an order for {{ order.total.formatted }} on {{ order.placed_at.formatted }}.{% if order.payment %} Payment: {{ order.payment.method }} ({{ order.payment.status }}).{% endif %}
{%- if admin_url %}

View the order: {{ admin_url }}
{%- endif %}

Items:
${text.orderLines}

${text.totals}
${text.delivery}

${text.footer}
`,
};
