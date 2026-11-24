import type { ReactNode } from "react";
import { Column, Row, Section, Text } from "react-email";
import type { Template } from "../email.ts";
import { For, If, v } from "../jinja.tsx";
import { colours, heading, muted, paragraph, preformatted, subheading } from "../styles.ts";
import * as text from "../text.ts";

const lineRow = { borderBottom: `1px solid ${colours.border}` };
const cell = { padding: "10px 0", verticalAlign: "top" };
const amount = { ...paragraph, margin: 0, whiteSpace: "nowrap" } as const;

/** Text the store wrote, line breaks kept, on a panel in its accent colour. */
function Panel(props: { path: string }) {
  return (
    <Section
      style={{
        backgroundColor: colours.panel,
        borderLeft: `3px solid ${v("brand.accent")}`,
        borderRadius: "4px",
        padding: "14px 16px",
        margin: "0 0 16px",
      }}
    >
      <pre style={preformatted}>{v(props.path)}</pre>
    </Section>
  );
}

/** One "label: value" detail of the order, when `test` (the value or its object) is there. */
function Detail(props: { label: string; path: string; test?: string }) {
  return (
    <If test={props.test ?? props.path}>
      <Text style={{ ...paragraph, margin: "0 0 4px" }}>
        <span style={{ color: colours.muted }}>{props.label}:</span> {v(props.path)}
      </Text>
    </If>
  );
}

function TotalRow(props: { label: ReactNode; value: string; strong?: boolean }) {
  const style = props.strong
    ? { ...paragraph, margin: 0, fontWeight: 700 }
    : { ...paragraph, margin: 0, color: colours.muted };
  return (
    <Row>
      <Column style={{ padding: "3px 0" }}>
        <Text style={style}>{props.label}</Text>
      </Column>
      <Column style={{ padding: "3px 0" }} align="right">
        <Text style={{ ...style, whiteSpace: "nowrap" }}>{props.value}</Text>
      </Column>
    </Row>
  );
}

/** An optional part of the ship-to address, on its own line when it's there. */
function AddressPart(props: { name: string }) {
  return (
    <If test={`purchase_order.ship_to.address.${props.name}`} inline>
      {v(`purchase_order.ship_to.address.${props.name}`)}
      <br />
    </If>
  );
}

export const supplierPurchaseOrder: Template = {
  key: "supplier_purchase_order",
  translated: false,
  email: () => ({
    key: "supplier_purchase_order",
    subject:
      "Purchase order {{ purchase_order.number }} from {{ shop.name }}{% if purchase_order.revision %} (revision {{ purchase_order.revision }}){% endif %}",
    preheader:
      "{{ shop.name }}'s order {{ purchase_order.number }} for {{ purchase_order.total.formatted }}.",
    Content: () => (
      <>
        <Text style={heading}>Purchase order {v("purchase_order.number")}</Text>
        <Text style={paragraph}>
          Hello {v("supplier.name")}, here is our purchase order
          <If test="purchase_order.revision" inline>
            {" "}
            (revision {v("purchase_order.revision")}, replacing the earlier one)
          </If>
          . Please confirm it and tell us if anything can't be delivered as listed.
        </Text>
        <If test="message">
          <Panel path="message" />
        </If>
        <Detail
          label="Ordered"
          path="purchase_order.ordered_at.formatted"
          test="purchase_order.ordered_at"
        />
        <Detail
          label="Expected by"
          path="purchase_order.expected_at.formatted"
          test="purchase_order.expected_at"
        />
        <Detail label="Your reference" path="purchase_order.supplier_reference" />
        <If test="purchase_order.payment_terms">
          <Text style={{ ...paragraph, margin: "0 0 4px" }}>
            <span style={{ color: colours.muted }}>Payment terms:</span>{" "}
            {v("purchase_order.payment_terms.summary")}
            <If test="purchase_order.payment_terms.note" inline>
              {" "}
              ({v("purchase_order.payment_terms.note")})
            </If>
          </Text>
        </If>
        <Text style={subheading}>Items</Text>
        <Section>
          <For each="line in purchase_order.lines">
            <Row style={lineRow}>
              <Column style={cell}>
                <Text style={{ ...paragraph, margin: 0 }}>{v("line.title")}</Text>
                <If test="line.variant">
                  <Text style={muted}>{v("line.variant")}</Text>
                </If>
                <If test="line.supplier_sku or line.sku">
                  <Text style={muted}>
                    <If test="line.supplier_sku" inline>
                      Your code {v("line.supplier_sku")}
                    </If>
                    <If test="line.supplier_sku and line.sku" inline>
                      {" · "}
                    </If>
                    <If test="line.sku" inline>
                      Our SKU {v("line.sku")}
                    </If>
                  </Text>
                </If>
              </Column>
              <Column style={{ ...cell, width: "56px" }} align="right">
                <Text style={{ ...muted, fontSize: "15px", lineHeight: "24px" }}>
                  × {v("line.quantity")}
                </Text>
              </Column>
              <Column style={{ ...cell, width: "130px" }} align="right">
                <Text style={amount}>{v("line.total.formatted")}</Text>
                <Text style={{ ...muted, margin: 0 }}>{v("line.unit_cost.formatted")} each</Text>
              </Column>
            </Row>
          </For>
        </Section>
        <Section style={{ marginTop: "12px" }}>
          <TotalRow label="Subtotal" value={v("purchase_order.subtotal.formatted")} />
          <For each="cost in purchase_order.costs">
            <TotalRow
              label={
                <>
                  {v("cost.label")}
                  <If test="cost.description" inline>
                    {" "}
                    ({v("cost.description")})
                  </If>
                </>
              }
              value={v("cost.amount.formatted")}
            />
          </For>
          <If test="purchase_order.tax">
            <TotalRow label="Tax" value={v("purchase_order.tax.formatted")} />
          </If>
          <TotalRow label="Total" value={v("purchase_order.total.formatted")} strong />
        </Section>
        <If test="purchase_order.ship_to">
          <Text style={subheading}>Ship to</Text>
          <Text style={{ ...paragraph, margin: "0 0 8px" }}>
            {v("purchase_order.ship_to.name")}
            <br />
            <If test="purchase_order.ship_to.address" inline>
              <AddressPart name="contact" />
              <AddressPart name="company" />
              {v("purchase_order.ship_to.address.line1")}
              <br />
              <AddressPart name="line2" />
              <If test="purchase_order.ship_to.address.postal_code" inline>
                {v("purchase_order.ship_to.address.postal_code")}{" "}
              </If>
              {v("purchase_order.ship_to.address.city")}
              <br />
              <AddressPart name="province" />
              {v("purchase_order.ship_to.address.country")}
              <If test="purchase_order.ship_to.address.phone" inline>
                <br />
                {v("purchase_order.ship_to.address.phone")}
              </If>
            </If>
          </Text>
        </If>
        <If test="purchase_order.note_to_supplier">
          <Text style={subheading}>Notes</Text>
          <Panel path="purchase_order.note_to_supplier" />
        </If>
      </>
    ),
    text: `Purchase order {{ purchase_order.number }}{% if purchase_order.revision %} (revision {{ purchase_order.revision }}, replacing the earlier one){% endif %}

Hello {{ supplier.name }}, here is our purchase order. Please confirm it and tell us if anything can't be delivered as listed.
{%- if message %}

{{ message }}
{%- endif %}
{%- if purchase_order.ordered_at %}

Ordered: {{ purchase_order.ordered_at.formatted }}
{%- endif %}
{%- if purchase_order.expected_at %}
Expected by: {{ purchase_order.expected_at.formatted }}
{%- endif %}
{%- if purchase_order.supplier_reference %}
Your reference: {{ purchase_order.supplier_reference }}
{%- endif %}
{%- if purchase_order.payment_terms %}
Payment terms: {{ purchase_order.payment_terms.summary }}{% if purchase_order.payment_terms.note %} ({{ purchase_order.payment_terms.note }}){% endif %}
{%- endif %}

Items:
{%- for line in purchase_order.lines %}
- {{ line.quantity }} × {{ line.title }}{% if line.variant %} ({{ line.variant }}){% endif %}{% if line.supplier_sku %}, your code {{ line.supplier_sku }}{% endif %}{% if line.sku %}, our SKU {{ line.sku }}{% endif %}: {{ line.unit_cost.formatted }} each, {{ line.total.formatted }}
{%- endfor %}

Subtotal: {{ purchase_order.subtotal.formatted }}
{%- for cost in purchase_order.costs %}
{{ cost.label }}{% if cost.description %} ({{ cost.description }}){% endif %}: {{ cost.amount.formatted }}
{%- endfor %}
{%- if purchase_order.tax %}
Tax: {{ purchase_order.tax.formatted }}
{%- endif %}
Total: {{ purchase_order.total.formatted }}
{%- if purchase_order.ship_to %}

Ship to:
{{ purchase_order.ship_to.name }}
{%- if purchase_order.ship_to.address %}
{%- if purchase_order.ship_to.address.contact %}
{{ purchase_order.ship_to.address.contact }}
{%- endif %}
{%- if purchase_order.ship_to.address.company %}
{{ purchase_order.ship_to.address.company }}
{%- endif %}
{{ purchase_order.ship_to.address.line1 }}
{%- if purchase_order.ship_to.address.line2 %}
{{ purchase_order.ship_to.address.line2 }}
{%- endif %}
{% if purchase_order.ship_to.address.postal_code %}{{ purchase_order.ship_to.address.postal_code }} {% endif %}{{ purchase_order.ship_to.address.city }}
{%- if purchase_order.ship_to.address.province %}
{{ purchase_order.ship_to.address.province }}
{%- endif %}
{{ purchase_order.ship_to.address.country }}
{%- if purchase_order.ship_to.address.phone %}
{{ purchase_order.ship_to.address.phone }}
{%- endif %}
{%- endif %}
{%- endif %}
{%- if purchase_order.note_to_supplier %}

Notes:
{{ purchase_order.note_to_supplier }}
{%- endif %}

${text.footer}
`,
  }),
};
