import { Text } from "react-email";
import type { Email } from "../email.ts";
import { v } from "../jinja.tsx";
import { Delivery, Greeting, OrderButton, OrderLines, Payment, Totals } from "../parts.tsx";
import { heading, muted, paragraph, subheading } from "../styles.ts";
import * as text from "../text.ts";

export const orderConfirmation: Email = {
  key: "order_confirmation",
  subject: "Order {{ order.number }} confirmed",
  preheader: "Thanks for your order. Here's what you ordered and what happens next.",
  Content: () => (
    <>
      <Text style={heading}>Thanks for your order</Text>
      <Greeting />
      <Text style={paragraph}>
        We've received your order {v("order.number")} and we're getting it ready. We'll email you
        again when it ships.
      </Text>
      <Payment />
      <OrderButton />
      <Text style={subheading}>Order {v("order.number")}</Text>
      <Text style={muted}>Placed {v("order.placed_at.formatted")}</Text>
      <OrderLines />
      <Totals />
      <Delivery />
    </>
  ),
  text: `${text.greeting}

We've received your order {{ order.number }} and we're getting it ready. We'll email you again when it ships.
${text.payment}
${text.orderLink}

Order {{ order.number }}, placed {{ order.placed_at.formatted }}
${text.orderLines}

${text.totals}
${text.delivery}

${text.footer}
`,
};
