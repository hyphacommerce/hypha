// The Hypha theme (brand book v0.1, October 2026): Foxfire lime on moss-grey neutrals,
// Geist and Geist Mono. One theme on Astryx among many: it extends Neutral and only
// replaces what the brand changes; a merchant theme can replace it the same way.
//
// Build after editing: `pnpm theme:build` (writes hypha.css, hypha.js, hypha.d.ts).
// `pnpm theme:check` fails while they're stale.

import { defineTheme, type TokenValue } from "@astryxdesign/core/theme";
import { neutralTheme } from "@astryxdesign/theme-neutral";

/** A palette's steps, 50 to 950, from the brand book. */
type Scale = Record<50 | 100 | 200 | 300 | 400 | 500 | 600 | 700 | 800 | 900 | 950, string>;

function scale(hexes: string[]): Scale {
  const steps = [50, 100, 200, 300, 400, 500, 600, 700, 800, 900, 950] as const;
  return Object.fromEntries(steps.map((step, i) => [step, hexes[i]])) as Scale;
}

/** Foxfire: brand-400 is the fill, always with brand-950 ink on it. */
const brand = scale([
  "#F5FEE8", "#EAFBCF", "#D9F7A5", "#C6F073", "#B8E948", "#A2D128",
  "#7EA602", "#5F7E01", "#455C01", "#304204", "#1D2904",
]);
/** Greys with a faint moss tint (oklch chroma 0.006, hue 130). */
const neutral = scale([
  "#FAFAF9", "#F3F4F2", "#E6E8E5", "#D4D5D2", "#A3A5A1", "#6A6C68",
  "#575955", "#444642", "#30322F", "#232522", "#1A1B19",
]);
/** Emerald, 30° away from brand and darker, so success never reads as brand. */
const success = scale([
  "#EDFBF1", "#D9F5E1", "#BAECCA", "#91DBAB", "#5EC386", "#24A965",
  "#028B4F", "#02703F", "#01532D", "#003B1E", "#002712",
]);
const warning = scale([
  "#FFF5EA", "#FFE9D1", "#FDD7AC", "#F1BC7E", "#DF9B44", "#C47D04",
  "#A06604", "#825100", "#613C01", "#462900", "#2F1A00",
]);
const danger = scale([
  "#FFF4F3", "#FEE7E4", "#FFD1CD", "#FFB0A9", "#F28881", "#E85854",
  "#C8393A", "#9A3936", "#752725", "#521D1B", "#371210",
]);
const info = scale([
  "#F0F8FF", "#DEEFFF", "#C2E2FF", "#95CEFE", "#5DB2F7", "#3496DE",
  "#037AC0", "#02629C", "#004975", "#003355", "#002139",
]);

/** A token's `[light, dark]` values. */
const pair = (light: string, dark: string): TokenValue => [light, dark];

/** The same for a component style, which takes CSS: `light-dark()`. */
const ld = (light: string, dark: string) => `light-dark(${light}, ${dark})`;

const fallbacks = "ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, sans-serif";

export const hyphaTheme = defineTheme({
  name: "hypha",
  extends: neutralTheme,

  // Typography replaces the base's config entirely, so it is complete here. The admin
  // is dense: a 13px base (the brand book's admin scale), Neutral's ratio and weights.
  typography: {
    scale: { base: 13, ratio: 1.2 },
    body: { family: "Geist Variable", fallbacks: `Geist, ${fallbacks}` },
    heading: {
      family: "Geist Variable",
      fallbacks: `Geist, ${fallbacks}`,
      weights: { 3: "bold", 4: "bold" },
    },
    code: {
      family: "Geist Mono Variable",
      fallbacks: '"Geist Mono", ui-monospace, SFMono-Regular, Menlo, monospace',
    },
  },

  tokens: {
    // Backgrounds. Dark mode starts at neutral-950, not black, and raises surfaces in
    // lightness steps (body → surface → muted/elevated) instead of shadows.
    "--color-background-body": pair(neutral[50], neutral[950]),
    "--color-background-surface": pair("#FFFFFF", neutral[900]),
    "--color-background-card": pair("#FFFFFF", neutral[900]),
    "--color-background-popover": pair("#FFFFFF", neutral[800]),
    "--color-background-muted": pair(neutral[100], neutral[800]),

    // Brand. Foxfire is a fill in both modes, with brand-950 ink on it; it is never text
    // on light grounds, where brand text is brand-800 (brand-300 on dark).
    "--color-accent": pair(brand[400], brand[400]),
    "--color-accent-muted": pair(brand[100], brand[900]),
    "--color-on-accent": pair(brand[950], brand[950]),
    "--color-text-accent": pair(brand[800], brand[300]),
    "--color-icon-accent": pair(brand[700], brand[300]),
    "--focus-outline-color": pair(brand[700], brand[400]),

    // Text and icons.
    "--color-text-primary": pair(neutral[950], neutral[50]),
    "--color-text-secondary": pair(neutral[700], neutral[300]),
    "--color-text-disabled": pair(neutral[400], neutral[600]),
    "--color-icon-primary": pair(neutral[950], neutral[50]),
    "--color-icon-secondary": pair(neutral[600], neutral[400]),
    "--color-icon-disabled": pair(neutral[400], neutral[600]),
    "--color-on-dark": neutral[50],
    "--color-on-light": neutral[950],

    // Status: fills with their text colour on them, and subtle backgrounds.
    "--color-success": pair(success[700], success[300]),
    "--color-success-muted": pair(success[50], success[900]),
    "--color-on-success": pair("#FFFFFF", neutral[950]),
    "--color-warning": pair(warning[700], warning[300]),
    "--color-warning-muted": pair(warning[50], warning[900]),
    "--color-on-warning": pair("#FFFFFF", neutral[950]),
    "--color-error": pair(danger[600], danger[400]),
    "--color-error-muted": pair(danger[50], danger[900]),
    "--color-on-error": pair("#FFFFFF", neutral[950]),

    // Borders.
    "--color-border": pair(neutral[200], neutral[800]),
    "--color-border-emphasized": pair(neutral[300], neutral[700]),

    // Elevation: Neutral's drops in light mode; in dark mode no drops, only a 1px
    // border-coloured rim, since shadows barely show on dark grounds.
    "--shadow-low":
      `0 2px 4px light-dark(oklch(0 0 0 / 5%), transparent), ` +
      `0 4px 8px light-dark(oklch(0 0 0 / 10%), transparent), ` +
      `inset 0 0 0 1px light-dark(transparent, ${neutral[800]})`,
    "--shadow-med":
      `0 2px 4px light-dark(oklch(0 0 0 / 5%), transparent), ` +
      `0 4px 12px light-dark(oklch(0 0 0 / 10%), transparent), ` +
      `inset 0 0 0 1px light-dark(transparent, ${neutral[700]})`,
    "--shadow-high":
      `0 4px 6px light-dark(oklch(0 0 0 / 10%), transparent), ` +
      `0 12px 24px light-dark(oklch(0 0 0 / 15%), transparent), ` +
      `inset 0 0 0 1px light-dark(transparent, ${neutral[700]})`,
    "--shadow-inset-hover": `inset 0px 0px 0px 2px ${brand[700]}4D`,
    "--shadow-inset-selected": `inset 0px 0px 0px 2px ${brand[700]}80`,
  },

  components: {
    // Status badges: a subtle background with the status colour as text, like the
    // brand book's commerce statuses (meeting AA in both modes).
    badge: {
      "variant:info": { backgroundColor: ld(info[50], info[900]), color: ld(info[700], info[300]) },
      "variant:success": {
        backgroundColor: ld(success[50], success[900]),
        color: ld(success[700], success[300]),
      },
      "variant:warning": {
        backgroundColor: ld(warning[50], warning[900]),
        color: ld(warning[700], warning[300]),
      },
      "variant:error": {
        backgroundColor: ld(danger[50], danger[900]),
        color: ld(danger[700], danger[300]),
      },
    },
  },
});
