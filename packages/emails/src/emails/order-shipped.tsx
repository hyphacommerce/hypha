import { Text } from "react-email";
import type { Email } from "../email.ts";
import { v } from "../jinja.tsx";
import { Delivery, Greeting, OrderButton, ShippedLines, Tracking } from "../parts.tsx";
import { heading, paragraph, subheading } from "../styles.ts";
import * as text from "../text.ts";

export const orderShipped: Email = {
  key: "order_shipped",
  subject: "Your order {{ order.number }} is on its way",
  preheader: "Items from your order {{ order.number }} have shipped.",
  Content: () => (
    <>
      <Text style={heading}>Your order is on its way</Text>
      <Greeting />
      <Text style={paragraph}>
        Good news: items from your order {v("order.number")} have shipped.
      </Text>
      <Tracking />
      <OrderButton />
      <Text style={subheading}>In this shipment</Text>
      <ShippedLines />
      <Delivery />
    </>
  ),
  text: `${text.greeting}

Good news: items from your order {{ order.number }} have shipped.
${text.tracking}

In this shipment:
${text.shippedLines}
${text.orderLink}
${text.delivery}

${text.footer}
`,
};
