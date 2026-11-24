import { Text } from "react-email";
import { j, type Template } from "../email.ts";
import { v } from "../jinja.tsx";
import { Delivery, Greeting, OrderButton, OrderLines, Payment, Totals } from "../parts.tsx";
import { heading, muted, paragraph, subheading } from "../styles.ts";
import * as text from "../text.ts";

export const orderConfirmation: Template = {
  key: "order_confirmation",
  translated: true,
  email: (t) => ({
    key: "order_confirmation",
    subject: t("order_confirmation.subject", { number: j("order.number") }),
    preheader: t("order_confirmation.preheader"),
    Content: () => (
      <>
        <Text style={heading}>{t("order_confirmation.heading")}</Text>
        <Greeting t={t} />
        <Text style={paragraph}>
          {t("order_confirmation.received", { number: v("order.number") })}
        </Text>
        <Payment t={t} />
        <OrderButton t={t} />
        <Text style={subheading}>{t("common.order_number", { number: v("order.number") })}</Text>
        <Text style={muted}>
          {t("order_confirmation.placed", { date: v("order.placed_at.formatted") })}
        </Text>
        <OrderLines />
        <Totals t={t} />
        <Delivery t={t} />
      </>
    ),
    text: `${text.greeting(t)}

${t("order_confirmation.received", { number: j("order.number") })}
${text.payment(t)}
${text.orderLink(t)}

${t("order_confirmation.text_placed", { number: j("order.number"), date: j("order.placed_at.formatted") })}
${text.orderLines(t)}

${text.totals(t)}
${text.delivery(t)}

${text.footer}
`,
  }),
};
