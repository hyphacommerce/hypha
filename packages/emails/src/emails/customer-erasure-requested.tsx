import { Text } from "react-email";
import { j, type Template } from "../email.ts";
import { v } from "../jinja.tsx";
import { heading, muted, paragraph } from "../styles.ts";
import * as text from "../text.ts";

export const customerErasureRequested: Template = {
  key: "customer_erasure_requested",
  translated: true,
  email: (t) => ({
    key: "customer_erasure_requested",
    subject: t("customer_erasure_requested.subject", { shop: j("shop.name") }),
    preheader: t("customer_erasure_requested.preheader", { date: j("due_at.formatted") }),
    Content: () => (
      <>
        <Text style={heading}>{t("customer_erasure_requested.heading")}</Text>
        <Text style={paragraph}>
          {t("customer_erasure_requested.body", {
            shop: v("shop.name"),
            date: v("due_at.formatted"),
          })}
        </Text>
        <Text style={paragraph}>{t("customer_erasure_requested.changed_mind")}</Text>
        <Text style={{ ...muted, marginTop: "16px" }}>
          {t("customer_erasure_requested.not_you")}
        </Text>
      </>
    ),
    text: `${t("customer_erasure_requested.body", { shop: j("shop.name"), date: j("due_at.formatted") })}

${t("customer_erasure_requested.changed_mind")}

${t("customer_erasure_requested.not_you")}

${text.footer}
`,
  }),
};
