// The plain-text bodies' shared parts, written as Jinja directly: a plain-text email
// should read like a note, not like HTML with the tags stripped.
//
// Whitespace: every tag that starts a line is written `{%- … %}`, which removes the line
// break before it, and every optional part starts with its own line breaks, so a part
// that's left out leaves no blank lines behind. This relies on minijinja's defaults
// (`trim_blocks` and `lstrip_blocks` off).

import { j } from "./email.ts";
import type { T } from "./i18n.ts";

export const greeting = (t: T) =>
  `{% if customer and customer.name %}${t("common.greeting_named", { name: j("customer.name") })}{% else %}${t("common.greeting")}{% endif %}`;

export const orderLines = (t: T) => `{%- for line in order.lines %}
- {{ line.quantity }} × {{ line.title }}{% if line.variant %} ({{ line.variant }}){% endif %}: {{ line.total.formatted }}{% if line.discount %} ${t("common.was", { amount: j("line.original_total.formatted") })}{% endif %}
{%- endfor %}`;

export const shippedLines = `{%- for line in fulfillment.lines %}
- {{ line.quantity }} × {{ line.title }}{% if line.variant %} ({{ line.variant }}){% endif %}
{%- endfor %}`;

// The lines' totals and shipping are after discounts, so the discounts are listed as
// what they include, not taken off again.
export const totals = (t: T) => `${t("common.subtotal")}: {{ order.subtotal.formatted }}
${t("common.shipping")}: {{ order.shipping.formatted }}
{%- if not order.taxes_included %}
${t("common.tax")}: {{ order.tax.formatted }}
{%- endif %}
${t("common.total")}: {{ order.total.formatted }}
{%- if order.taxes_included %}
${t("common.including_tax", { amount: j("order.tax.formatted") })}
{%- endif %}
{%- if order.discounts %}

${t("common.discounts_included")}:
{%- for discount in order.discounts %}
- {{ discount.title }}{% if discount.code %} ({{ discount.code }}){% endif %}: {{ discount.amount.formatted }}
{%- endfor %}
{%- endif %}`;

export const delivery = (t: T) => `{%- if order.shipping_address %}

${t("common.shipping_to")}:
{%- if order.shipping_address.name %}
{{ order.shipping_address.name }}
{%- endif %}
{%- if order.shipping_address.company %}
{{ order.shipping_address.company }}
{%- endif %}
{{ order.shipping_address.line1 }}
{%- if order.shipping_address.line2 %}
{{ order.shipping_address.line2 }}
{%- endif %}
{% if order.shipping_address.postal_code %}{{ order.shipping_address.postal_code }} {% endif %}{{ order.shipping_address.city }}
{%- if order.shipping_address.province %}
{{ order.shipping_address.province }}
{%- endif %}
{{ order.shipping_address.country }}
{%- if order.shipping_address.phone %}
{{ order.shipping_address.phone }}
{%- endif %}
{%- endif %}
{%- if order.shipping_method %}

${t("common.shipping_method")}: {{ order.shipping_method }}
{%- endif %}`;

export const payment = (t: T) => `{%- if order.payment %}

${t("common.payment")}: {{ order.payment.method }}
{%- if order.payment.instructions %}

{{ order.payment.instructions }}
{%- endif %}
{%- endif %}`;

export const tracking = (t: T) => `{%- if fulfillment.tracking %}

${t("common.tracking")}:
{%- for tracking in fulfillment.tracking %}
- {% if tracking.carrier %}{{ tracking.carrier }}: {% endif %}{{ tracking.number }}{% if tracking.url %} ({{ tracking.url }}){% endif %}
{%- endfor %}
{%- endif %}`;

export const orderLink = (t: T) => `{%- if order.url %}

${t("common.view_order")}: {{ order.url }}
{%- endif %}`;

/** The text version of the layout's footer: the store's text, name and address. */
export const footer = `--
{{ shop.name }}{% if shop.email %} · {{ shop.email }}{% endif %}
{%- if brand.footer %}
{{ brand.footer }}
{%- endif %}`;
