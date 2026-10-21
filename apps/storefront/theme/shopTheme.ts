// The shop's theme (design note 0024, AU12): a warm, editorial look for shoppers, apart
// from the admin's Hypha theme. Clay on cream, Fraunces headings over Instrument Sans,
// a roomier type scale. Extends Neutral and only replaces what the shop changes, so a
// merchant forking the storefront restyles it here.
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

/** Clay: clay-700 is the fill with white ink (5.6:1); clay-300 the fill on dark. */
const clay = scale([
  "#FDF4EF",
  "#FAE5DA",
  "#F3C9B3",
  "#E9A584",
  "#DD7F57",
  "#CC6339",
  "#B4502A",
  "#953F21",
  "#77331D",
  "#5C2918",
  "#3A190E",
]);
/** Warm stone greys. */
const stone = scale([
  "#FBF8F3",
  "#F4EFE7",
  "#E8E1D6",
  "#D5CCBE",
  "#ABA192",
  "#7D7466",
  "#635B4F",
  "#4D463C",
  "#36312A",
  "#26221D",
  "#1A1714",
]);
const moss = scale([
  "#F1F7EE",
  "#DEEDD8",
  "#BFDCB3",
  "#97C586",
  "#6DA95A",
  "#4E8E3C",
  "#3C742D",
  "#305C25",
  "#26481E",
  "#1C3517",
  "#11220E",
]);
const amber = scale([
  "#FFF7E8",
  "#FEEBC8",
  "#FBD48D",
  "#F4B852",
  "#E49B24",
  "#C47F10",
  "#A0650C",
  "#7F4F0D",
  "#613D0F",
  "#472D0C",
  "#2D1C07",
]);
const rose = scale([
  "#FDF2F2",
  "#FBE3E3",
  "#F5C3C3",
  "#EC9B9B",
  "#E06E6E",
  "#CC4B4B",
  "#B03737",
  "#8F2D2D",
  "#712626",
  "#561E1E",
  "#371212",
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
    body: {
      family: "Instrument Sans Variable",
      // "… Fallback": the system font sized to its metrics (globals.css), until it's in.
      fallbacks: `"Instrument Sans Fallback", "Instrument Sans", ${sans}`,
    },
    heading: {
      family: "Fraunces Variable",
      fallbacks: '"Fraunces Fallback", Fraunces, Georgia, "Times New Roman", serif',
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

    "--color-accent": pair(clay[700], clay[300]),
    "--color-accent-muted": pair(clay[100], clay[900]),
    "--color-on-accent": pair("#FFFFFF", clay[950]),
    "--color-text-accent": pair(clay[700], clay[300]),
    "--color-icon-accent": pair(clay[700], clay[300]),
    "--focus-outline-color": pair(clay[700], clay[300]),

    "--color-text-primary": pair(stone[950], stone[50]),
    "--color-text-secondary": pair(stone[600], stone[300]),
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

    "--shadow-inset-hover": `inset 0px 0px 0px 2px ${clay[700]}4D`,
    "--shadow-inset-selected": `inset 0px 0px 0px 2px ${clay[700]}`,
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
