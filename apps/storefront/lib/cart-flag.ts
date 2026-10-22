// Whether the shopper has a cart in a country, for the browser. The cart's ID stays in an
// httpOnly cookie; this one, readable by page scripts, only says that there is one, so a
// static page reads the cart (`loadCart`, an API call) only when it can find something. It
// holds no secret: a forged one costs the shopper's own read.

/** The flag's cookie, next to the cart's (`cart_hr`): `has_cart_hr=1`. */
export function cartFlagCookie(country: string): string {
  return `has_cart_${country.toLowerCase()}`;
}

/** Whether the browser's cookies say there's a cart in `country`. Client only. */
export function hasCartFlag(country: string): boolean {
  const name = `${cartFlagCookie(country)}=`;
  return document.cookie.split(";").some((part) => part.trim().startsWith(name));
}
