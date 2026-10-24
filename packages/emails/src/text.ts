// The plain-text bodies' shared parts, written as Jinja directly: a plain-text email
// should read like a note, not like HTML with the tags stripped.
//
// Whitespace: every tag that starts a line is written `{%- … %}`, which removes the line
// break before it, and every optional part starts with its own line breaks, so a part
// that's left out leaves no blank lines behind. This relies on minijinja's defaults
// (`trim_blocks` and `lstrip_blocks` off).

export const greeting = "Hi{% if customer and customer.name %} {{ customer.name }}{% endif %},";

export const orderLines = `{%- for line in order.lines %}
- {{ line.quantity }} × {{ line.title }}{% if line.variant %} ({{ line.variant }}){% endif %}: {{ line.total.formatted }}{% if line.discount %} (was {{ line.original_total.formatted }}){% endif %}
{%- endfor %}`;

export const shippedLines = `{%- for line in fulfillment.lines %}
- {{ line.quantity }} × {{ line.title }}{% if line.variant %} ({{ line.variant }}){% endif %}
{%- endfor %}`;

// The lines' totals and shipping are after discounts, so the discounts are listed as
// what they include, not taken off again.
export const totals = `Subtotal: {{ order.subtotal.formatted }}
Shipping: {{ order.shipping.formatted }}
{%- if not order.taxes_included %}
Tax: {{ order.tax.formatted }}
{%- endif %}
Total: {{ order.total.formatted }}
{%- if order.taxes_included %}
Including {{ order.tax.formatted }} in taxes
{%- endif %}
{%- if order.discounts %}

Discounts included above:
{%- for discount in order.discounts %}
- {{ discount.title }}{% if discount.code %} ({{ discount.code }}){% endif %}: {{ discount.amount.formatted }}
{%- endfor %}
{%- endif %}`;

export const delivery = `{%- if order.shipping_address %}

Shipping to:
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

Shipping method: {{ order.shipping_method }}
{%- endif %}`;

export const payment = `{%- if order.payment %}

Payment: {{ order.payment.method }}
{%- if order.payment.instructions %}

{{ order.payment.instructions }}
{%- endif %}
{%- endif %}`;

export const tracking = `{%- if fulfillment.tracking %}

Tracking:
{%- for tracking in fulfillment.tracking %}
- {% if tracking.carrier %}{{ tracking.carrier }}: {% endif %}{{ tracking.number }}{% if tracking.url %} ({{ tracking.url }}){% endif %}
{%- endfor %}
{%- endif %}`;

export const orderLink = `{%- if order.url %}

View your order: {{ order.url }}
{%- endif %}`;

/** The text version of the layout's footer: the store's text, name and address. */
export const footer = `--
{{ shop.name }}{% if shop.email %} · {{ shop.email }}{% endif %}
{%- if brand.footer %}
{{ brand.footer }}
{%- endif %}`;
