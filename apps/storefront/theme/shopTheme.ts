// The shop's theme: Hypha's look, like hyphacommerce.com and the admin (Foxfire lime on
// moss-grey neutrals, Geist), with a roomier type scale for shoppers. Extends Neutral
// and only replaces what the shop changes, so a merchant forking the storefront restyles
// it here.
//
// Build after editing: `pnpm theme:build` (writes shop.css, shop.js, shop.d.ts).
// `pnpm theme:check` fails while they're stale.

import { defineTheme, type TokenValue } from "@astryxdesign/core/theme";
import { neutralTheme } from "@astryxdesign/theme-neutral";

/** A palette's steps, 50 to 950. */
type Scale = Record<50 | 100 | 200 | 300 | 400 | 500 | 600 | 700 | 800 | 900 | 950, string>;

/** One hex per step, so a palette with a step missing doesn't compile. */
type Hexes = [
  string,
  string,
  string,
  string,
  string,
  string,
  string,
  string,
  string,
  string,
  string,
];

function scale([s50, s100, s200, s300, s400, s500, s600, s700, s800, s900, s950]: Hexes): Scale {
  return {
    50: s50,
    100: s100,
    200: s200,
    300: s300,
    400: s400,
    500: s500,
    600: s600,
    700: s700,
    800: s800,
    900: s900,
    950: s950,
  };
}

/** Foxfire, Hypha's lime (brand book v0.1): brand-400 is the fill, always with brand-950
 * ink on it; it's never text on light grounds, where brand text is brand-800. */
const brand = scale([
  "#F5FEE8",
  "#EAFBCF",
  "#D9F7A5",
  "#C6F073",
  "#B8E948",
  "#A2D128",
  "#7EA602",
  "#5F7E01",
  "#455C01",
  "#304204",
  "#1D2904",
]);
/** Greys with a faint moss tint, the same as hyphacommerce.com's and the admin's. */
const stone = scale([
  "#FAFAF9",
  "#F3F4F2",
  "#E6E8E5",
  "#D4D5D2",
  "#A3A5A1",
  "#6A6C68",
  "#575955",
  "#444642",
  "#30322F",
  "#232522",
  "#1A1B19",
]);
const moss = scale([
  "#EDFBF1",
  "#D9F5E1",
  "#BAECCA",
  "#91DBAB",
  "#5EC386",
  "#24A965",
  "#028B4F",
  "#02703F",
  "#01532D",
  "#003B1E",
  "#002712",
]);
const amber = scale([
  "#FFF5EA",
  "#FFE9D1",
  "#FDD7AC",
  "#F1BC7E",
  "#DF9B44",
  "#C47D04",
  "#A06604",
  "#825100",
  "#613C01",
  "#462900",
  "#2F1A00",
]);
const rose = scale([
  "#FFF4F3",
  "#FEE7E4",
  "#FFD1CD",
  "#FFB0A9",
  "#F28881",
  "#E85854",
  "#C8393A",
  "#9A3936",
  "#752725",
  "#521D1B",
  "#371210",
]);

/** A token's `[light, dark]` values. */
const pair = (light: string, dark: string): TokenValue => [light, dark];

/** The same for a component style, which takes CSS: `light-dark()`. */
const ld = (light: string, dark: string) => `light-dark(${light}, ${dark})`;

const sans = "ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, sans-serif";

export const shopTheme = defineTheme({
  name: "shop",
  extends: neutralTheme,

  // A shop reads at a comfortable size: a 15px base and a wider ratio, so product
  // titles carry the page.
  typography: {
    scale: { base: 15, ratio: 1.25 },
    body: { family: "Geist Variable", fallbacks: `Geist, ${sans}` },
    heading: {
      family: "Geist Variable",
      fallbacks: `Geist, ${sans}`,
      weights: { 1: "semibold", 2: "semibold", 3: "semibold", 4: "semibold" },
    },
    code: {
      family: "ui-monospace",
      fallbacks: "SFMono-Regular, Menlo, monospace",
    },
  },

  // Softer than Neutral's corners on cards and images, tighter on controls.
  radius: { base: 4, multiplier: 1.25 },

  tokens: {
    "--color-background-body": pair(stone[50], stone[950]),
    "--color-background-surface": pair("#FFFFFF", stone[900]),
    "--color-background-card": pair("#FFFFFF", stone[900]),
    "--color-background-popover": pair("#FFFFFF", stone[800]),
    "--color-background-muted": pair(stone[100], stone[800]),

    "--color-accent": pair(brand[400], brand[400]),
    "--color-accent-muted": pair(brand[100], brand[900]),
    "--color-on-accent": pair(brand[950], brand[950]),
    "--color-text-accent": pair(brand[800], brand[300]),
    "--color-icon-accent": pair(brand[700], brand[300]),
    "--focus-outline-color": pair(brand[700], brand[400]),

    "--color-text-primary": pair(stone[950], stone[50]),
    "--color-text-secondary": pair(stone[700], stone[300]),
    "--color-text-disabled": pair(stone[400], stone[600]),
    "--color-icon-primary": pair(stone[950], stone[50]),
    "--color-icon-secondary": pair(stone[600], stone[400]),
    "--color-icon-disabled": pair(stone[400], stone[600]),
    "--color-on-dark": stone[50],
    "--color-on-light": stone[950],

    "--color-success": pair(moss[700], moss[300]),
    "--color-success-muted": pair(moss[50], moss[900]),
    "--color-on-success": pair("#FFFFFF", stone[950]),
    "--color-warning": pair(amber[700], amber[300]),
    "--color-warning-muted": pair(amber[50], amber[900]),
    "--color-on-warning": pair("#FFFFFF", stone[950]),
    "--color-error": pair(rose[600], rose[400]),
    "--color-error-muted": pair(rose[50], rose[900]),
    "--color-on-error": pair("#FFFFFF", stone[950]),

    "--color-border": pair(stone[200], stone[800]),
    "--color-border-emphasized": pair(stone[300], stone[700]),

    "--shadow-inset-hover": `inset 0px 0px 0px 2px ${brand[700]}4D`,
    "--shadow-inset-selected": `inset 0px 0px 0px 2px ${brand[700]}`,
  },

  components: {
    badge: {
      "variant:success": {
        backgroundColor: ld(moss[50], moss[900]),
        color: ld(moss[700], moss[300]),
      },
      "variant:warning": {
        backgroundColor: ld(amber[50], amber[900]),
        color: ld(amber[700], amber[300]),
      },
      "variant:error": {
        backgroundColor: ld(rose[50], rose[900]),
        color: ld(rose[700], rose[300]),
      },
    },
  },
});
