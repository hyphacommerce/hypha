import { Text } from "react-email";
import { j, type Template } from "../email.ts";
import { v } from "../jinja.tsx";
import { Greeting, OrderButton, ShippedLines, Tracking } from "../parts.tsx";
import { heading, paragraph, subheading } from "../styles.ts";
import * as text from "../text.ts";

export const orderTrackingUpdated: Template = {
  key: "order_tracking_updated",
  translated: true,
  email: (t) => ({
    key: "order_tracking_updated",
    subject: t("order_tracking_updated.subject", { number: j("order.number") }),
    preheader: t("order_tracking_updated.preheader", { number: j("order.number") }),
    Content: () => (
      <>
        <Text style={heading}>{t("order_tracking_updated.heading")}</Text>
        <Greeting t={t} />
        <Text style={paragraph}>
          {t("order_tracking_updated.changed", { number: v("order.number") })}
        </Text>
        <Tracking t={t} />
        <OrderButton t={t} />
        <Text style={subheading}>{t("common.in_shipment")}</Text>
        <ShippedLines />
      </>
    ),
    text: `${text.greeting(t)}

${t("order_tracking_updated.changed", { number: j("order.number") })}
${text.tracking(t)}

${t("common.in_shipment")}:
${text.shippedLines}
${text.orderLink(t)}

${text.footer}
`,
  }),
};
