// Pieces the order emails share: the greeting, the button, lines, totals, the address,
// the payment and the tracking. Each reads the context's names.

import type { ReactNode } from "react";
import { Button, Column, Link, Row, Section, Text } from "react-email";
import { For, If, v } from "./jinja.tsx";
import { colours, muted, paragraph, preformatted, subheading } from "./styles.ts";

/** "Hi Ana," with the customer's name, "Hi," without. */
export function Greeting() {
  return (
    <If test="customer and customer.name" otherwise={<Text style={paragraph}>Hi,</Text>}>
      <Text style={paragraph}>Hi {v("customer.name")},</Text>
    </If>
  );
}

/** A button in the store's accent colour. */
export function PrimaryButton(props: { href: string; children: ReactNode }) {
  return (
    <Section style={{ margin: "8px 0 24px" }}>
      <Button
        href={props.href}
        style={{
          backgroundColor: v("brand.accent"),
          color: "#ffffff",
          fontSize: "15px",
          fontWeight: 600,
          borderRadius: "6px",
          padding: "12px 22px",
        }}
      >
        {props.children}
      </Button>
    </Section>
  );
}

/** The order page's button, when the store set an order link. */
export function OrderButton() {
  return (
    <If test="order.url">
      <PrimaryButton href={v("order.url")}>View your order</PrimaryButton>
    </If>
  );
}

function Variant(props: { name: string }) {
  return (
    <If test={`${props.name}.variant`}>
      <Text style={muted}>{v(`${props.name}.variant`)}</Text>
    </If>
  );
}

const lineRow = { borderBottom: `1px solid ${colours.border}` };
const cell = { padding: "10px 0", verticalAlign: "top" };
const amount = { ...paragraph, margin: 0, whiteSpace: "nowrap" } as const;

/** The order's lines: title and variant, quantity, line total. */
export function OrderLines() {
  return (
    <Section>
      <For each="line in order.lines">
        <Row style={lineRow}>
          <Column style={cell}>
            <Text style={{ ...paragraph, margin: 0 }}>{v("line.title")}</Text>
            <Variant name="line" />
          </Column>
          <Column style={{ ...cell, width: "56px" }} align="right">
            <Text style={{ ...muted, fontSize: "15px", lineHeight: "24px" }}>
              × {v("line.quantity")}
            </Text>
          </Column>
          <Column style={{ ...cell, width: "110px" }} align="right">
            <Text style={amount}>{v("line.total.formatted")}</Text>
            <If test="line.discount">
              <Text style={{ ...muted, margin: 0, textDecoration: "line-through" }}>
                {v("line.original_total.formatted")}
              </Text>
            </If>
          </Column>
        </Row>
      </For>
    </Section>
  );
}

/** What a fulfilment shipped: title and variant, quantity. */
export function ShippedLines() {
  return (
    <Section>
      <For each="line in fulfillment.lines">
        <Row style={lineRow}>
          <Column style={cell}>
            <Text style={{ ...paragraph, margin: 0 }}>{v("line.title")}</Text>
            <Variant name="line" />
          </Column>
          <Column style={{ ...cell, width: "56px" }} align="right">
            <Text style={{ ...muted, fontSize: "15px", lineHeight: "24px" }}>
              × {v("line.quantity")}
            </Text>
          </Column>
        </Row>
      </For>
    </Section>
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

/**
 * Subtotal (the lines' totals, after discounts), shipping (after discounts), tax (a line
 * of its own, or included in the total) and total, which add up; then the discounts they
 * already include, so none is taken off twice.
 */
export function Totals() {
  return (
    <Section style={{ marginTop: "12px" }}>
      <TotalRow label="Subtotal" value={v("order.subtotal.formatted")} />
      <TotalRow label="Shipping" value={v("order.shipping.formatted")} />
      <If test="not order.taxes_included">
        <TotalRow label="Tax" value={v("order.tax.formatted")} />
      </If>
      <TotalRow label="Total" value={v("order.total.formatted")} strong />
      <If test="order.taxes_included">
        <Text style={{ ...muted, textAlign: "right" }}>
          Including {v("order.tax.formatted")} in taxes
        </Text>
      </If>
      <If test="order.discounts">
        <Text style={{ ...muted, margin: "12px 0 0" }}>Discounts included above</Text>
        <For each="discount in order.discounts">
          <TotalRow
            label={
              <>
                {v("discount.title")}
                <If test="discount.code" inline>
                  {" "}
                  ({v("discount.code")})
                </If>
              </>
            }
            value={v("discount.amount.formatted")}
          />
        </For>
      </If>
    </Section>
  );
}

/** An optional part of the shipping address, on its own line when it's there. */
function AddressPart(props: { name: string }) {
  return (
    <If test={`order.shipping_address.${props.name}`} inline>
      {v(`order.shipping_address.${props.name}`)}
      <br />
    </If>
  );
}

/** The shipping address, one part per line, and the shipping method. */
export function Delivery() {
  return (
    <If test="order.shipping_address or order.shipping_method">
      <Text style={subheading}>Delivery</Text>
      <If test="order.shipping_address">
        <Text style={{ ...paragraph, margin: "0 0 8px" }}>
          <AddressPart name="name" />
          <AddressPart name="company" />
          {v("order.shipping_address.line1")}
          <br />
          <AddressPart name="line2" />
          <If test="order.shipping_address.postal_code" inline>
            {v("order.shipping_address.postal_code")}{" "}
          </If>
          {v("order.shipping_address.city")}
          <br />
          <AddressPart name="province" />
          {v("order.shipping_address.country")}
          <If test="order.shipping_address.phone" inline>
            <br />
            {v("order.shipping_address.phone")}
          </If>
        </Text>
      </If>
      <If test="order.shipping_method">
        <Text style={{ ...muted, fontSize: "14px" }}>{v("order.shipping_method")}</Text>
      </If>
    </If>
  );
}

/** The payment method and its instructions (a bank transfer's details), line breaks kept. */
export function Payment() {
  return (
    <If test="order.payment">
      <Text style={subheading}>Payment</Text>
      <Text style={{ ...paragraph, margin: "0 0 8px" }}>{v("order.payment.method")}</Text>
      <If test="order.payment.instructions">
        <Section
          style={{
            backgroundColor: colours.panel,
            borderLeft: `3px solid ${v("brand.accent")}`,
            borderRadius: "4px",
            padding: "14px 16px",
            margin: "0 0 8px",
          }}
        >
          <pre style={preformatted}>{v("order.payment.instructions")}</pre>
        </Section>
      </If>
    </If>
  );
}

/** Each tracking number, with its carrier and a link to the carrier's page when there's one. */
export function Tracking() {
  return (
    <If test="fulfillment.tracking">
      <Text style={subheading}>Tracking</Text>
      <For each="tracking in fulfillment.tracking">
        <Text style={{ ...paragraph, margin: "0 0 6px" }}>
          <If test="tracking.carrier" inline>
            {v("tracking.carrier")}:{" "}
          </If>
          <If test="tracking.url" inline otherwise={v("tracking.number")}>
            <Link href={v("tracking.url")} style={{ color: v("brand.accent") }}>
              {v("tracking.number")}
            </Link>
          </If>
        </Text>
      </For>
    </If>
  );
}

/** A link written out, for clients that don't show the button. */
export function PlainLink(props: { href: string }) {
  return (
    <Text style={muted}>
      If the button doesn't work, copy this link into your browser:
      <br />
      <Link href={props.href} style={{ color: v("brand.accent"), wordBreak: "break-all" }}>
        {props.href}
      </Link>
    </Text>
  );
}
