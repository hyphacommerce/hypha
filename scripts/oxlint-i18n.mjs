// An Oxlint JS plugin for the apps whose words come from catalogs (`@hyphacommerce/i18n`):
// `i18n/no-literal-text` refuses text written into JSX, which would be English in every
// language. Enabled per app in the root `vite.config.ts`.
//
// It reports text between tags (`<p>Hello</p>`, `<p>{"Hello"}</p>`, a template literal's
// own words) and the props people read (`READ_PROPS`: HTML's, Astryx's and the apps' own).
// Text without a letter (punctuation, symbols, numbers) passes: no language translates
// " · ".

/** The props whose value a person reads or hears, on elements and on components. */
const READ_PROPS = new Set([
  // HTML and ARIA.
  "alt",
  "aria-description",
  "aria-label",
  "placeholder",
  "title",
  // Astryx's components and the apps' own.
  "actionLabel",
  "addLabel",
  "alreadyInLabel",
  "cancelLabel",
  "caption",
  "confirmLabel",
  "description",
  "disabledMessage",
  "emptyBootstrapText",
  "heading",
  "helperText",
  "label",
  "note",
  "noun",
  "searchLabel",
  "submitLabel",
  "subtitle",
  "tooltip",
]);

const LETTER = /\p{L}/u;

/** The literal text of an expression, if it has any: a string, or a template's own parts. */
function literalText(node) {
  if (!node) return "";
  if (node.type === "Literal") return typeof node.value === "string" ? node.value : "";
  if (node.type === "TemplateLiteral") {
    return node.quasis.map((part) => part.value.cooked ?? part.value.raw).join("");
  }
  if (node.type === "ConditionalExpression") {
    return literalText(node.consequent) + literalText(node.alternate);
  }
  if (node.type === "LogicalExpression") return literalText(node.left) + literalText(node.right);
  return "";
}

const noLiteralText = {
  meta: {
    type: "problem",
    docs: { description: "Text in JSX comes from the catalogs, through `t`." },
    messages: {
      text: "Literal text {{text}} in JSX: take it from the catalogs (`t(…)`), so it's translated.",
    },
    schema: [],
  },
  create(context) {
    const report = (node, text) => {
      if (!LETTER.test(text)) return;
      const shown = JSON.stringify(text.trim().replace(/\s+/g, " ").slice(0, 40));
      context.report({ node, messageId: "text", data: { text: shown } });
    };
    return {
      JSXText(node) {
        report(node, node.value);
      },
      JSXExpressionContainer(node) {
        // A prop's expression is the attribute's to judge, below.
        if (node.parent?.type === "JSXAttribute") return;
        report(node, literalText(node.expression));
      },
      JSXAttribute(node) {
        const name = node.name.type === "JSXIdentifier" ? node.name.name : "";
        if (!READ_PROPS.has(name) || !node.value) return;
        const value =
          node.value.type === "JSXExpressionContainer" ? node.value.expression : node.value;
        report(node, literalText(value));
      },
    };
  },
};

export default {
  meta: { name: "i18n" },
  rules: { "no-literal-text": noLiteralText },
};
