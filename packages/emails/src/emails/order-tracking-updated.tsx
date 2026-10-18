import { Text } from "react-email";
import type { Email } from "../email.ts";
import { v } from "../jinja.tsx";
import { Greeting, OrderButton, ShippedLines, Tracking } from "../parts.tsx";
import { heading, paragraph, subheading } from "../styles.ts";
import * as text from "../text.ts";

export const orderTrackingUpdated: Email = {
  key: "order_tracking_updated",
  subject: "Tracking updated for order {{ order.number }}",
  preheader: "There's new tracking information for your order {{ order.number }}.",
  Content: () => (
    <>
      <Text style={heading}>Tracking updated</Text>
      <Greeting />
      <Text style={paragraph}>
        The tracking information for a shipment from your order {v("order.number")} has changed.
        Here are the latest details.
      </Text>
      <Tracking />
      <OrderButton />
      <Text style={subheading}>In this shipment</Text>
      <ShippedLines />
    </>
  ),
  text: `${text.greeting}

The tracking information for a shipment from your order {{ order.number }} has changed. Here are the latest details.
${text.tracking}

In this shipment:
${text.shippedLines}
${text.orderLink}

${text.footer}
`,
};
