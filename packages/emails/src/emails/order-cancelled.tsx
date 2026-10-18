import { Text } from "react-email";
import type { Email } from "../email.ts";
import { If, v } from "../jinja.tsx";
import { Greeting, OrderButton, OrderLines, Totals } from "../parts.tsx";
import { heading, paragraph, subheading } from "../styles.ts";
import * as text from "../text.ts";

export const orderCancelled: Email = {
  key: "order_cancelled",
  subject: "Order {{ order.number }} cancelled",
  preheader: "Your order {{ order.number }} has been cancelled.",
  Content: () => (
    <>
      <Text style={heading}>Your order has been cancelled</Text>
      <Greeting />
      <Text style={paragraph}>
        Your order {v("order.number")} has been cancelled.
        <If test="cancellation.reason" inline>
          {" "}
          Reason: {v("cancellation.reason")}.
        </If>
      </Text>
      <Text style={paragraph}>If you have any questions about it, just reply to this email.</Text>
      <OrderButton />
      <Text style={subheading}>Order {v("order.number")}</Text>
      <OrderLines />
      <Totals />
    </>
  ),
  text: `${text.greeting}

Your order {{ order.number }} has been cancelled.{% if cancellation.reason %} Reason: {{ cancellation.reason }}.{% endif %}

If you have any questions about it, just reply to this email.
${text.orderLink}

Order {{ order.number }}
${text.orderLines}

${text.totals}

${text.footer}
`,
};
