// The defaults' look: neutral greys around the store's accent colour (`brand.accent`).
// Inline styles only, since email clients drop most stylesheets.

import type { CSSProperties } from "react";

export const font =
  '-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif';

export const colours = {
  page: "#f4f4f5",
  card: "#ffffff",
  text: "#18181b",
  muted: "#71717a",
  border: "#e4e4e7",
  panel: "#f4f4f5",
};

export const heading: CSSProperties = {
  fontSize: "22px",
  lineHeight: "30px",
  fontWeight: 700,
  color: colours.text,
  margin: "8px 0 16px",
};

export const subheading: CSSProperties = {
  fontSize: "15px",
  lineHeight: "22px",
  fontWeight: 600,
  color: colours.text,
  margin: "24px 0 8px",
};

export const paragraph: CSSProperties = {
  fontSize: "15px",
  lineHeight: "24px",
  color: colours.text,
  margin: "0 0 16px",
};

export const muted: CSSProperties = {
  fontSize: "13px",
  lineHeight: "20px",
  color: colours.muted,
  margin: 0,
};

/** Text with its line breaks kept (payment instructions, the footer): `<pre>`, in the body's font. */
export const preformatted: CSSProperties = {
  whiteSpace: "pre-wrap",
  fontFamily: font,
  fontSize: "14px",
  lineHeight: "22px",
  color: colours.text,
  margin: 0,
};
