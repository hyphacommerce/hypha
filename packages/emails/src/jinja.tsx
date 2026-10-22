// Jinja in React Email. The templates are JSX, but what the API stores is Jinja, so the
// JSX carries markers that survive React's rendering and become Jinja afterwards:
//
// - `v("order.number")` is a variable: it returns a placeholder (`__JV0__`) usable as
//   text and in attributes (an `href`, an `src`, a style's colour). Placeholders are
//   letters, digits and underscores only, so React escapes nothing in them.
// - `<If test="…">`, `<For each="line in order.lines">` and `<Block name="content">`
//   render custom elements (`<jinja-if data-jinja="0">…</jinja-if>`) that `toJinja`
//   turns into `{% if … %}…{% endif %}` and the like.
//
// Expressions are registered per render (`collect`), so the markers carry only an index;
// the expression itself never passes through React.

import { createElement, type ReactNode } from "react";
import { pretty, render } from "react-email";

let registry: string[] = [];

/** A dotted path (`order.payment.instructions`) or a `for` head (`line in order.lines`). */
const PATH = /^[a-z_][a-z0-9_]*(\.[a-z_][a-z0-9_]*)*$/;
const TEST = /^[a-z0-9_. ]+$/;
const EACH = /^([a-z_][a-z0-9_]*) in ([a-z_][a-z0-9_]*(\.[a-z_][a-z0-9_]*)*)$/;

function register(expression: string, rule: RegExp, what: string): string {
  if (!rule.test(expression)) throw new Error(`not a ${what}: ${JSON.stringify(expression)}`);
  registry.push(expression);
  return String(registry.length - 1);
}

/** A variable, printed with `{{ … }}` (autoescaped on the API's side). */
export function v(path: string): string {
  return `__JV${register(path, PATH, "variable path")}__`;
}

type Marker = "if" | "for" | "block";

function marker(kind: Marker, index: string, inline: boolean, ...children: ReactNode[]): ReactNode {
  const props: Record<string, string> = { "data-jinja": index };
  if (inline) props["data-inline"] = "";
  return createElement(`jinja-${kind}`, props, ...children);
}

/**
 * `{% if test %}children{% else %}otherwise{% endif %}`. `test` is a Jinja expression of
 * paths, `and`, `or` and `not` (`order.payment and order.payment.instructions`). `inline`
 * for one inside a line of text (an address's optional line), so it isn't laid out on
 * lines of its own.
 */
export function If(props: {
  test: string;
  children: ReactNode;
  otherwise?: ReactNode;
  inline?: boolean;
}) {
  const index = register(props.test, TEST, "test");
  const inline = props.inline === true;
  if (props.otherwise === undefined) return marker("if", index, inline, props.children);
  const separator = createElement("jinja-else", inline ? { "data-inline": "" } : {});
  return marker("if", index, inline, props.children, separator, props.otherwise);
}

/** `{% for each %}children{% endfor %}`, `each` as `line in order.lines`. */
export function For(props: { each: string; children: ReactNode }) {
  return marker("for", register(props.each, EACH, "for head"), false, props.children);
}

/** `{% block name %}children{% endblock %}`: the layout's slots. */
export function Block(props: { name: string; children?: ReactNode; inline?: boolean }) {
  const index = register(props.name, PATH, "block name");
  return marker("block", index, props.inline === true, props.children);
}

/**
 * Renders an element to HTML with Jinja in it: React Email's renderer, Prettier (so the
 * defaults read well in the admin's code editor) and the markers replaced. `document`
 * keeps the doctype (the layout); otherwise the result is a fragment (a block's content).
 */
export async function toJinja(element: ReactNode, options: { document: boolean }): Promise<string> {
  registry = [];
  const rendered = await render(<>{element}</>);
  const expressions = registry;
  let html = options.document ? rendered : rendered.replace(/^<!DOCTYPE[^>]*>/, "");
  // React's own comments (text separators, Suspense and document boundaries) only matter
  // to hydration, which an email never has.
  html = html.replace(/<!--(?: |\$|\/\$|html|head|body)-->/g, "");
  // React escapes apostrophes everywhere; only single-quoted attributes need it, and
  // React quotes attributes with double quotes. "We've" reads better in the editor.
  html = html.replaceAll("&#x27;", "'");
  // Prettier treats unknown elements as inline text and hugs them; a display comment
  // makes it lay each marker out on its own lines, like a block.
  html = html.replace(
    /<jinja-(if|for|block|else)(?![^>]*data-inline)/g,
    "<!-- display: block --><jinja-$1",
  );
  html = await pretty(html, { printWidth: 100 });
  html = html.replace(/<!-- display: block -->\s*/g, "").replace(/\s+data-inline(?:="")?/g, "");

  const expression = (index: string): string => {
    const found = expressions[Number(index)];
    if (found === undefined) throw new Error(`unknown marker ${index}`);
    return found;
  };
  const tags: Record<string, string> = { if: "if", for: "for", block: "block" };
  html = html
    .replace(
      /<jinja-(if|for|block)\s+data-jinja="(\d+)"\s*>/g,
      (_, kind: string, index: string) => {
        return `{% ${tags[kind]} ${expression(index)} %}`;
      },
    )
    .replace(/<\/jinja-(if|for|block)\s*>/g, (_, kind: string) => `{% end${tags[kind]} %}`)
    .replace(/<jinja-else\s*>\s*<\/jinja-else\s*>/g, "{% else %}")
    .replace(/__JV(\d+)__/g, (_, index: string) => `{{ ${expression(index)} }}`);
  html = indentTags(html);
  const leftover = /jinja-|__JV/.exec(html);
  if (leftover)
    throw new Error(
      `a marker survived rendering near: ${html.slice(leftover.index - 40, leftover.index + 40)}`,
    );
  return html.trimEnd() + "\n";
}

/**
 * Puts each `{% else %}` and closing tag that stands on its own line at its opening tag's
 * indentation (Prettier indents the markers like the elements they were).
 */
function indentTags(html: string): string {
  const split = html.replace(/^([ \t]*)(\S.*?)\{% else %\}[ \t]*$/gm, "$1$2\n$1{% else %}");
  const open: string[] = [];
  return split
    .split("\n")
    .map((line) => {
      const tag = /^([ \t]*)\{% (\w+)[^%]*%\}$/.exec(line);
      if (!tag) return line;
      const [, indent = "", name = ""] = tag;
      if (name === "if" || name === "for" || name === "block") {
        open.push(indent);
        return line;
      }
      const opening = open.at(-1);
      if (opening === undefined) return line;
      if (name.startsWith("end")) open.pop();
      return opening + line.trimStart();
    })
    .join("\n");
}
