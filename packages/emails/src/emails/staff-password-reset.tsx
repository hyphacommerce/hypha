import { Text } from "react-email";
import type { Email } from "../email.ts";
import { v } from "../jinja.tsx";
import { PlainLink, PrimaryButton } from "../parts.tsx";
import { heading, muted, paragraph } from "../styles.ts";
import * as text from "../text.ts";

export const staffPasswordReset: Email = {
  key: "staff_password_reset",
  subject: "Reset your {{ shop.name }} password",
  preheader: "Choose a new password for {{ user.email }}.",
  Content: () => (
    <>
      <Text style={heading}>Reset your password</Text>
      <Text style={paragraph}>
        Someone asked to reset the password of {v("user.email")} for {v("shop.name")}. Choose a new
        one with the button below.
      </Text>
      <PrimaryButton href={v("link")}>Choose a new password</PrimaryButton>
      <Text style={paragraph}>The link expires on {v("expires_at.formatted")}.</Text>
      <PlainLink href={v("link")} />
      <Text style={{ ...muted, marginTop: "16px" }}>
        If you didn't ask for this, you can ignore this email: your password stays the same.
      </Text>
    </>
  ),
  text: `Someone asked to reset the password of {{ user.email }} for {{ shop.name }}. Choose a new one here:

{{ link }}

The link expires on {{ expires_at.formatted }}.

If you didn't ask for this, you can ignore this email: your password stays the same.

${text.footer}
`,
};
