import { Text } from "react-email";
import type { Email } from "../email.ts";
import { v } from "../jinja.tsx";
import { PlainLink, PrimaryButton } from "../parts.tsx";
import { heading, muted, paragraph } from "../styles.ts";
import * as text from "../text.ts";

export const staffInvite: Email = {
  key: "staff_invite",
  subject: "You're invited to {{ shop.name }}",
  preheader: "Accept the invitation to start working on {{ shop.name }}.",
  Content: () => (
    <>
      <Text style={heading}>You're invited to {v("shop.name")}</Text>
      <Text style={paragraph}>
        You've been invited to help run {v("shop.name")}. Accept the invitation to choose a password
        and sign in as {v("user.email")}.
      </Text>
      <PrimaryButton href={v("link")}>Accept the invitation</PrimaryButton>
      <Text style={paragraph}>The invitation expires on {v("expires_at.formatted")}.</Text>
      <PlainLink href={v("link")} />
      <Text style={{ ...muted, marginTop: "16px" }}>
        If you weren't expecting this invitation, you can ignore this email.
      </Text>
    </>
  ),
  text: `You've been invited to help run {{ shop.name }}. Accept the invitation to choose a password and sign in as {{ user.email }}:

{{ link }}

The invitation expires on {{ expires_at.formatted }}.

If you weren't expecting this invitation, you can ignore this email.

${text.footer}
`,
};
