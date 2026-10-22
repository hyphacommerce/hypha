// Every default template rendered to its Jinja files: the layout and, per key, the
// subject, the HTML (the layout's blocks) and the plain text.

import type { Email } from "./email.ts";
import { customerErasureRequested } from "./emails/customer-erasure-requested.tsx";
import { customerLoginCode } from "./emails/customer-login-code.tsx";
import { orderCancelled } from "./emails/order-cancelled.tsx";
import { orderConfirmation } from "./emails/order-confirmation.tsx";
import { orderShipped } from "./emails/order-shipped.tsx";
import { orderTrackingUpdated } from "./emails/order-tracking-updated.tsx";
import { staffInvite } from "./emails/staff-invite.tsx";
import { staffNewOrder } from "./emails/staff-new-order.tsx";
import { staffPasswordReset } from "./emails/staff-password-reset.tsx";
import { toJinja } from "./jinja.tsx";
import { Layout } from "./layout.tsx";

export const emails: Email[] = [
  orderConfirmation,
  orderShipped,
  orderTrackingUpdated,
  orderCancelled,
  customerLoginCode,
  customerErasureRequested,
  staffNewOrder,
  staffInvite,
  staffPasswordReset,
];

function oneLine(email: Email, what: string, value: string): string {
  // Written into HTML as is (the layout's preheader), so no markup or entities.
  if (/[\n<>&]/.test(value))
    throw new Error(`${email.key}'s ${what} must be one line of plain text`);
  return value;
}

/** File name (in `crates/hypha-notifications/defaults/`) → contents. */
export async function renderAll(): Promise<Map<string, string>> {
  const files = new Map<string, string>();
  files.set("layout.html.j2", await toJinja(<Layout />, { document: true }));
  for (const email of emails) {
    const { Content } = email;
    const content = await toJinja(<Content />, { document: false });
    files.set(`${email.key}.subject.j2`, oneLine(email, "subject", email.subject));
    files.set(
      `${email.key}.html.j2`,
      `{% extends "layout" %}\n\n` +
        `{% block preheader %}${oneLine(email, "preheader", email.preheader)}{% endblock %}\n\n` +
        `{% block content %}\n${content}{% endblock %}\n`,
    );
    files.set(`${email.key}.txt.j2`, email.text);
  }
  return files;
}
