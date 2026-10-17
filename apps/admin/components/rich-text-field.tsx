"use client";

import { RichTextEditor, RichTextEditorToolbar } from "@astryxdesign/richtext";
import { $generateHtmlFromNodes, $generateNodesFromDOM } from "@lexical/html";
import { useLexicalComposerContext } from "@lexical/react/LexicalComposerContext";
import { $getRoot, $insertNodes, type LexicalEditor } from "lexical";
import { useEffect, useRef } from "react";

/** The update tag of loading a value in, which isn't an edit. */
const LOAD = "hypha-load";

/**
 * An HTML field edited as rich text (design note 0024, AU9): the value is HTML in and
 * out, as the API stores it, and the API still sanitises what's saved. Controlled like
 * a `TextArea`: `onChange` gets the new HTML ("" when empty) only when someone edits,
 * so loading a description doesn't count as a change. A new `value` from outside (a
 * discard, what a save stored) replaces the content.
 */
export function RichTextField({
  label,
  description,
  value,
  onChange,
  isOptional,
  minHeight = 160,
}: {
  label: string;
  description?: string;
  value: string;
  onChange: (html: string) => void;
  isOptional?: boolean;
  minHeight?: number;
}) {
  return (
    <RichTextEditor
      label={label}
      description={description}
      isOptional={isOptional}
      minHeight={minHeight}
      // h1 is the page's own title (the product's or category's name).
      toolbar={<RichTextEditorToolbar headingLevels={["h2", "h3"]} />}
      plugins={<HtmlValuePlugin value={value} onChange={onChange} />}
    />
  );
}

/** Keeps the editor and the HTML value in step, both ways. */
function HtmlValuePlugin({ value, onChange }: { value: string; onChange: (html: string) => void }) {
  const [editor] = useLexicalComposerContext();
  // The HTML the editor holds: what it loaded or last reported, so the value coming back
  // from the parent isn't loaded again (that would move the cursor while typing).
  const holds = useRef<string | null>(null);
  const report = useRef(onChange);
  report.current = onChange;

  useEffect(() => {
    if (value === holds.current) return;
    holds.current = value;
    load(editor, value);
  }, [editor, value]);

  useEffect(
    () =>
      editor.registerUpdateListener(({ editorState, dirtyElements, dirtyLeaves, tags }) => {
        if (tags.has(LOAD) || (dirtyElements.size === 0 && dirtyLeaves.size === 0)) return;
        const html = editorState.read(() =>
          $getRoot().getTextContentSize() === 0 ? "" : tidy($generateHtmlFromNodes(editor, null)),
        );
        if (html === holds.current) return;
        holds.current = html;
        report.current(html);
      }),
    [editor],
  );

  return null;
}

function load(editor: LexicalEditor, html: string) {
  editor.update(
    () => {
      const root = $getRoot();
      root.clear();
      if (html.trim() === "") return;
      const dom = new DOMParser().parseFromString(html, "text/html");
      root.select();
      $insertNodes($generateNodesFromDOM(editor, dom));
    },
    // Not an edit, and not something to undo back past.
    { tag: [LOAD, "history-merge"] },
  );
}

/**
 * Lexical's HTML without its styling: the classes, inline styles and `dir` it adds, and
 * the spans left bare by removing them. The API would drop most of it anyway; tidying
 * here keeps what's saved the same as what loads back.
 */
function tidy(html: string): string {
  const doc = new DOMParser().parseFromString(`<body>${html}</body>`, "text/html");
  for (const el of Array.from(doc.body.querySelectorAll("*"))) {
    el.removeAttribute("class");
    el.removeAttribute("style");
    el.removeAttribute("dir");
    el.removeAttribute("value");
  }
  // Bold arrives as <b><strong>…</strong></b>, italics as <i><em>…</em></i>.
  for (const el of Array.from(doc.body.querySelectorAll("span, b > strong, i > em"))) {
    if (el.attributes.length === 0) {
      moveChildren(el, el.parentNode, el);
      el.remove();
    }
  }
  for (const el of Array.from(doc.body.querySelectorAll("b, i"))) {
    const replacement = doc.createElement(el.tagName === "B" ? "strong" : "em");
    moveChildren(el, replacement, null);
    el.replaceWith(replacement);
  }
  return doc.body.innerHTML;
}

/** Moves `from`'s children into `to`, before `before` (at the end for `null`). */
function moveChildren(from: Node, to: Node | null, before: Node | null) {
  while (from.firstChild) to?.insertBefore(from.firstChild, before);
}
