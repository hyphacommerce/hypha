// The CLI's version, from its package.json: what release-please bumps. The published
// images are pinned to it (images.ts), so a CLI deploys the API, admin and storefront
// released with it.

import pkg from "../package.json" with { type: "json" };

export const VERSION: string = pkg.version;
