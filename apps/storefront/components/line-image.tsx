// A cart line's thumbnail, shared by the cart (a client component) and the checkout and
// order summaries (server components): no hooks here, so either can render it.

/** A line's square thumbnail, or an empty tile of the same size. */
export function LineImage({ image, size = 64 }: { image?: string | null; size?: number }) {
  return (
    <div className="shop-rounded" style={{ width: size, height: size, flexShrink: 0 }}>
      {image && <img className="shop-fill" src={image} alt="" />}
    </div>
  );
}

/** The thumbnail URL of a cart line's image, if it's a picture. */
export function lineImage(
  image: { kind: string; url: string; thumbnail_url?: string | null } | null | undefined,
): string | null {
  return image?.kind === "image" ? (image.thumbnail_url ?? image.url) : null;
}
