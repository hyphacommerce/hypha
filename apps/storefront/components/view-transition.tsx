// React's `ViewTransition` and `addTransitionType` (in React 19.3), imported here
// alone: vinext rewrites any `import { … ViewTransition … } from "react"` to a shim for
// older Reacts, which loses the other named exports in the same statement in
// development (`export *` over the pre-bundled package). This `export … from` isn't
// rewritten, so components import them from here together with whatever else they need
// from "react".
export { ViewTransition, addTransitionType } from "react";

/** The type of a transition that changes something within a page (a listing's results,
 * a product's hero image), which the page's own boundary leaves alone: it only
 * cross-fades on a navigation, which the router starts without a type. */
export const WITHIN_PAGE = "shop-within-page";

/** The type of a transition that shows another of a product's images: the hero's
 * boundary cross-fades on it alone (a re-render for anything else, like the cart,
 * leaves it be). */
export const HERO_IMAGE = "shop-hero-image";
