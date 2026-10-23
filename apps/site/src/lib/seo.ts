import { EMAIL, GITHUB, SITE_DESCRIPTION, SITE_TITLE } from "../consts";

// Structured data (schema.org JSON-LD). Every page carries the organisation and the site;
// pages add their own node (the software on the home page, a post on its page), linked
// to those by `@id`.

const SITE = "https://hyphacommerce.com";
export const ORG_ID = `${SITE}/#organization`;
export const WEBSITE_ID = `${SITE}/#website`;

const organization = {
  "@type": "Organization",
  "@id": ORG_ID,
  name: SITE_TITLE,
  url: `${SITE}/`,
  logo: `${SITE}/apple-touch-icon.png`,
  email: EMAIL,
  sameAs: [GITHUB, "https://www.npmjs.com/org/hyphacommerce"],
};

const website = {
  "@type": "WebSite",
  "@id": WEBSITE_ID,
  name: SITE_TITLE,
  url: `${SITE}/`,
  description: SITE_DESCRIPTION,
  publisher: { "@id": ORG_ID },
  inLanguage: "en",
};

export const software = {
  "@type": "SoftwareApplication",
  "@id": `${SITE}/#software`,
  name: SITE_TITLE,
  description: SITE_DESCRIPTION,
  url: `${SITE}/`,
  applicationCategory: "BusinessApplication",
  applicationSubCategory: "E-commerce platform",
  operatingSystem: "Linux, Cloudflare Workers",
  softwareVersion: "0.x (public preview)",
  license: "https://github.com/hyphacommerce/hypha#license",
  offers: { "@type": "Offer", price: "0", priceCurrency: "USD" },
  publisher: { "@id": ORG_ID },
  sameAs: GITHUB,
};

export const sourceCode = {
  "@type": "SoftwareSourceCode",
  "@id": `${SITE}/#source`,
  name: "hypha",
  codeRepository: GITHUB,
  programmingLanguage: ["Rust", "TypeScript"],
  license: ["https://opensource.org/license/mit", "https://www.apache.org/licenses/LICENSE-2.0"],
  targetProduct: { "@id": `${SITE}/#software` },
};

export function graph(...nodes: object[]) {
  return { "@context": "https://schema.org", "@graph": [organization, website, ...nodes] };
}
