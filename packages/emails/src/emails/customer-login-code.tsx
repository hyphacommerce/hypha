import { Section, Text } from "react-email";
import { j, type Template } from "../email.ts";
import { If, v } from "../jinja.tsx";
import { PlainLink, PrimaryButton } from "../parts.tsx";
import { colours, heading, muted, paragraph } from "../styles.ts";
import * as text from "../text.ts";

export const customerLoginCode: Template = {
  key: "customer_login_code",
  translated: true,
  email: (t) => ({
    key: "customer_login_code",
    subject: t("customer_login_code.subject", { shop: j("shop.name") }),
    preheader: t("customer_login_code.preheader", { shop: j("shop.name") }),
    Content: () => (
      <>
        <Text style={heading}>{t("customer_login_code.heading")}</Text>
        <Text style={paragraph}>
          {t("customer_login_code.enter", {
            shop: v("shop.name"),
            minutes: v("expires_in_minutes"),
          })}
        </Text>
        <Section
          style={{
            backgroundColor: colours.panel,
            border: `1px solid ${colours.border}`,
            borderRadius: "8px",
            padding: "16px 20px",
            margin: "8px 0 24px",
          }}
        >
          <Text
            style={{
              fontSize: "32px",
              lineHeight: "40px",
              fontWeight: 700,
              letterSpacing: "0.3em",
              textAlign: "center",
              color: colours.text,
              margin: 0,
            }}
          >
            {v("code")}
          </Text>
        </Section>
        <If test="shop.url">
          <PrimaryButton href={v("link")}>{t("customer_login_code.sign_in")}</PrimaryButton>
          <PlainLink t={t} href={v("link")} />
        </If>
        <Text style={{ ...muted, marginTop: "16px" }}>{t("customer_login_code.ignore")}</Text>
      </>
    ),
    text: `${t("customer_login_code.enter", { shop: j("shop.name"), minutes: j("expires_in_minutes") })}

    {{ code }}
{%- if shop.url %}

${t("customer_login_code.one_click", { link: j("link") })}
{%- endif %}

${t("customer_login_code.ignore")}

${text.footer}
`,
  }),
};
