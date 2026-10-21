import { Section, Text } from "react-email";
import type { Email } from "../email.ts";
import { If, v } from "../jinja.tsx";
import { PlainLink, PrimaryButton } from "../parts.tsx";
import { colours, heading, muted, paragraph } from "../styles.ts";
import * as text from "../text.ts";

export const customerLoginCode: Email = {
  key: "customer_login_code",
  subject: "{{ code }} is your {{ shop.name }} sign-in code",
  preheader: "Enter this code to sign in to {{ shop.name }}.",
  Content: () => (
    <>
      <Text style={heading}>Your sign-in code</Text>
      <Text style={paragraph}>
        Enter this code to sign in to {v("shop.name")}. It works for {v("expires_in_minutes")}{" "}
        minutes.
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
        <PrimaryButton href={v("link")}>Sign in</PrimaryButton>
        <PlainLink href={v("link")} />
      </If>
      <Text style={{ ...muted, marginTop: "16px" }}>
        If you didn't ask for a code, you can ignore this email: nobody can sign in without it.
      </Text>
    </>
  ),
  text: `Enter this code to sign in to {{ shop.name }}. It works for {{ expires_in_minutes }} minutes.

    {{ code }}
{%- if shop.url %}

Or sign in with one click: {{ link }}
{%- endif %}

If you didn't ask for a code, you can ignore this email: nobody can sign in without it.

${text.footer}
`,
};
