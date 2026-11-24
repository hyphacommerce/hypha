import { Text } from "react-email";
import { j, type Template } from "../email.ts";
import { If, v } from "../jinja.tsx";
import { Greeting, OrderButton, OrderLines, Totals } from "../parts.tsx";
import { heading, paragraph, subheading } from "../styles.ts";
import * as text from "../text.ts";

export const orderCancelled: Template = {
  key: "order_cancelled",
  translated: true,
  email: (t) => ({
    key: "order_cancelled",
    subject: t("order_cancelled.subject", { number: j("order.number") }),
    preheader: t("order_cancelled.preheader", { number: j("order.number") }),
    Content: () => (
      <>
        <Text style={heading}>{t("order_cancelled.heading")}</Text>
        <Greeting t={t} />
        <Text style={paragraph}>
          {t("order_cancelled.cancelled", { number: v("order.number") })}
          <If test="cancellation.reason" inline>
            {" "}
            {t("order_cancelled.reason", { reason: v("cancellation.reason") })}
          </If>
        </Text>
        <Text style={paragraph}>{t("order_cancelled.questions")}</Text>
        <OrderButton t={t} />
        <Text style={subheading}>{t("common.order_number", { number: v("order.number") })}</Text>
        <OrderLines />
        <Totals t={t} />
      </>
    ),
    text: `${text.greeting(t)}

${t("order_cancelled.cancelled", { number: j("order.number") })}{% if cancellation.reason %} ${t("order_cancelled.reason", { reason: j("cancellation.reason") })}{% endif %}

${t("order_cancelled.questions")}
${text.orderLink(t)}

${t("common.order_number", { number: j("order.number") })}
${text.orderLines(t)}

${text.totals(t)}

${text.footer}
`,
  }),
};
