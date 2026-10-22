// The shared frame every default extends (`{% extends "layout" %}`): the store's logo or
// name, its accent colour as a bar along the top, the content, and a footer with the
// store's own text and its contact address. Merchants can edit it once to change every
// email.

import { Body, Container, Head, Hr, Html, Img, Link, Section, Text } from "react-email";
import { Block, If, v } from "./jinja.tsx";
import { colours, font, preformatted } from "./styles.ts";

// Clients show the start of the body next to the subject; the preheader is that text,
// and these invisible characters keep the body's first words from following it.
const filler = "‌ ​".repeat(60);

export function Layout() {
  return (
    <Html lang="en">
      <Head>
        <meta name="color-scheme" content="light" />
        <meta name="supported-color-schemes" content="light" />
        <title>{v("shop.name")}</title>
      </Head>
      <Body
        style={{ backgroundColor: colours.page, margin: 0, padding: "24px 0", fontFamily: font }}
      >
        <div
          style={{
            display: "none",
            overflow: "hidden",
            lineHeight: "1px",
            opacity: 0,
            maxHeight: 0,
            maxWidth: 0,
          }}
          data-skip-in-text="true"
        >
          <Block name="preheader" inline />
          {filler}
        </div>
        <Container
          style={{
            backgroundColor: colours.card,
            borderTop: `4px solid ${v("brand.accent")}`,
            borderRadius: "6px",
            maxWidth: "600px",
          }}
        >
          <Section style={{ padding: "28px 32px 8px" }}>
            <If
              test="brand.logo_url"
              otherwise={
                <Text style={{ fontSize: "20px", fontWeight: 700, color: colours.text, margin: 0 }}>
                  {v("shop.name")}
                </Text>
              }
            >
              <Img src={v("brand.logo_url")} alt={v("shop.name")} height="40" />
            </If>
          </Section>
          <Section style={{ padding: "8px 32px 24px" }}>
            <Block name="content" />
          </Section>
          <Section style={{ padding: "0 32px 28px" }}>
            <Hr style={{ borderColor: colours.border, margin: "0 0 20px" }} />
            <If test="brand.footer">
              <pre
                style={{
                  ...preformatted,
                  fontSize: "12px",
                  color: colours.muted,
                  margin: "0 0 8px",
                }}
              >
                {v("brand.footer")}
              </pre>
            </If>
            <Text style={{ fontSize: "12px", lineHeight: "18px", color: colours.muted, margin: 0 }}>
              {v("shop.name")}
              <If test="shop.email" inline>
                {" · "}
                <Link href={`mailto:${v("shop.email")}`} style={{ color: colours.muted }}>
                  {v("shop.email")}
                </Link>
              </If>
            </Text>
          </Section>
        </Container>
      </Body>
    </Html>
  );
}
