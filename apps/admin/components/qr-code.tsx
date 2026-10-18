// A QR code drawn as one SVG path from uqr's module matrix: no canvas, no HTML strings,
// and it renders on the server too. Always dark on white, whatever the theme, so phone
// cameras read it.

import { encode } from "uqr";

export function QrCode({
  value,
  size = 192,
  label,
}: {
  value: string;
  size?: number;
  label: string;
}) {
  // Medium error correction and a 4-module quiet zone, as the QR spec recommends.
  const { data, size: modules } = encode(value, { ecc: "M", border: 4 });
  let path = "";
  data.forEach((row, y) => {
    row.forEach((isDark, x) => {
      if (isDark) path += `M${x} ${y}h1v1h-1z`;
    });
  });
  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${modules} ${modules}" shape-rendering="crispEdges">` +
    `<rect width="${modules}" height="${modules}" fill="#fff"/><path d="${path}" fill="#000"/></svg>`;
  // An <img> of the SVG: an image to assistive tech, and nothing in it can run.
  return (
    <img
      src={`data:image/svg+xml,${encodeURIComponent(svg)}`}
      alt={label}
      width={size}
      height={size}
    />
  );
}
