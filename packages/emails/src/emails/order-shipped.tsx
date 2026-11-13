import { Text } from "react-email";
import { j, type Template } from "../email.ts";
import { v } from "../jinja.tsx";
import { Delivery, Greeting, OrderButton, ShippedLines, Tracking } from "../parts.tsx";
import { heading, paragraph, subheading } from "../styles.ts";
import * as text from "../text.ts";

export const orderShipped: Template = {
  key: "order_shipped",
  translated: true,
  email: (t) => ({
    key: "order_shipped",
    subject: t("order_shipped.subject", { number: j("order.number") }),
    preheader: t("order_shipped.preheader", { number: j("order.number") }),
    Content: () => (
      <>
        <Text style={heading}>{t("order_shipped.heading")}</Text>
        <Greeting t={t} />
        <Text style={paragraph}>{t("order_shipped.shipped", { number: v("order.number") })}</Text>
        <Tracking t={t} />
        <OrderButton t={t} />
        <Text style={subheading}>{t("common.in_shipment")}</Text>
        <ShippedLines />
        <Delivery t={t} />
      </>
    ),
    text: `${text.greeting(t)}

${t("order_shipped.shipped", { number: j("order.number") })}
${text.tracking(t)}

${t("common.in_shipment")}:
${text.shippedLines}
${text.orderLink(t)}
${text.delivery(t)}

${text.footer}
`,
  }),
};
