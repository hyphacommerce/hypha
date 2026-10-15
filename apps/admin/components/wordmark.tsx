// The Hypha wordmark (brand assets in public/brand): the ink version on light grounds,
// the on-dark one (Foxfire tip) on dark. Both are rendered and CSS in globals.css shows
// the one for the current colour mode: Astryx's `data-theme` on <html> when a mode is
// chosen, else the system preference. The SVGs are cropped tight, so the component adds
// the brand's clear space itself: half the mark's height on every side.

/** `height` in pixels; the brand's smallest wordmark is 64px wide (about 20px tall). */
export function Wordmark({ height = 20 }: { height?: number }) {
  // Sized in style: Astryx's reset (`height: auto`) would override the attribute.
  const style = { height, width: "auto" };
  return (
    <span style={{ display: "block", padding: height / 2 }}>
      <img className="hypha-wordmark-light" src="/brand/hypha-wordmark.svg" alt="Hypha" style={style} />
      <img
        className="hypha-wordmark-dark"
        src="/brand/hypha-wordmark-on-dark.svg"
        alt="Hypha"
        style={style}
      />
    </span>
  );
}
