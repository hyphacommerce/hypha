import { Text } from "react-email";
import type { Email } from "../email.ts";
import { v } from "../jinja.tsx";
import { heading, muted, paragraph } from "../styles.ts";
import * as text from "../text.ts";

export const customerErasureRequested: Email = {
  key: "customer_erasure_requested",
  subject: "Your request to erase your data at {{ shop.name }}",
  preheader: "Your personal data will be erased on {{ due_at.formatted }} unless you cancel.",
  Content: () => (
    <>
      <Text style={heading}>We'll erase your data</Text>
      <Text style={paragraph}>
        You asked {v("shop.name")} to erase your personal data. We'll do it on{" "}
        {v("due_at.formatted")}: your name, contact details, addresses and marketing choices will be
        removed from your account and your orders, and your account will be closed. Your orders
        themselves stay on record without anything that identifies you.
      </Text>
      <Text style={paragraph}>
        Changed your mind? Sign in to your account before then and cancel the request.
      </Text>
      <Text style={{ ...muted, marginTop: "16px" }}>
        If you didn't ask for this, sign in and cancel the request, or contact us.
      </Text>
    </>
  ),
  text: `You asked {{ shop.name }} to erase your personal data. We'll do it on {{ due_at.formatted }}: your name, contact details, addresses and marketing choices will be removed from your account and your orders, and your account will be closed. Your orders themselves stay on record without anything that identifies you.

Changed your mind? Sign in to your account before then and cancel the request.

If you didn't ask for this, sign in and cancel the request, or contact us.

${text.footer}
`,
};
