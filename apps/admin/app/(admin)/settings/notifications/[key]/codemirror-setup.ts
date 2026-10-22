// CodeMirror for a template's sources: HTML, Jinja's tags marked on top, and the line a
// preview or save failed on highlighted. Loaded on demand by `code-editor.tsx`, so the
// editor's code only reaches the pages that show one.

import { html } from "@codemirror/lang-html";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { type Extension, RangeSetBuilder, StateEffect, StateField } from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  EditorView,
  MatchDecorator,
  ViewPlugin,
  type ViewUpdate,
} from "@codemirror/view";
import { tags } from "@lezer/highlight";
import { basicSetup } from "codemirror";

/** Colours from Astryx's syntax tokens, so the editor follows the theme and dark mode. */
const highlight = HighlightStyle.define([
  { tag: tags.tagName, color: "var(--color-syntax-tag)" },
  { tag: tags.attributeName, color: "var(--color-syntax-attribute)" },
  { tag: [tags.string, tags.attributeValue], color: "var(--color-syntax-string)" },
  { tag: tags.comment, color: "var(--color-syntax-comment)", fontStyle: "italic" },
  { tag: [tags.angleBracket, tags.punctuation], color: "var(--color-syntax-punctuation)" },
  { tag: tags.propertyName, color: "var(--color-syntax-property)" },
  { tag: [tags.number, tags.unit], color: "var(--color-syntax-number)" },
  { tag: tags.keyword, color: "var(--color-syntax-keyword)" },
]);

/** `{{ … }}`, `{% … %}` and `{# … #}`: Jinja over the HTML. */
const jinjaMarks = new MatchDecorator({
  regexp: /\{\{.*?\}\}|\{%.*?%\}|\{#.*?#\}/g,
  decoration: (match) =>
    Decoration.mark({ class: match[0].startsWith("{#") ? "cm-jinja-comment" : "cm-jinja" }),
});

const jinja = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;
    constructor(view: EditorView) {
      this.decorations = jinjaMarks.createDeco(view);
    }
    update(update: ViewUpdate) {
      this.decorations = jinjaMarks.updateDeco(update, this.decorations);
    }
  },
  { decorations: (plugin) => plugin.decorations },
);

/** Which line (from 1) to mark as the problem's; `null` clears it. */
const setErrorLine = StateEffect.define<number | null>();

const errorLine = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(marks, tr) {
    let next = marks.map(tr.changes);
    for (const effect of tr.effects) {
      if (!effect.is(setErrorLine)) continue;
      const builder = new RangeSetBuilder<Decoration>();
      const line = effect.value;
      if (line !== null && line >= 1 && line <= tr.state.doc.lines) {
        const at = tr.state.doc.line(line).from;
        builder.add(at, at, Decoration.line({ class: "cm-error-line" }));
      }
      next = builder.finish();
    }
    return next;
  },
  provide: (field) => EditorView.decorations.from(field),
});

const theme = EditorView.theme({
  "&": {
    fontSize: "13px",
    border: "1px solid var(--color-border)",
    borderRadius: "8px",
    backgroundColor: "var(--color-background-card)",
    color: "var(--color-text-primary)",
  },
  "&.cm-focused": { outline: "2px solid var(--color-accent)", outlineOffset: "-1px" },
  ".cm-scroller": { fontFamily: "var(--font-family-code)", lineHeight: "1.5" },
  ".cm-gutters": {
    backgroundColor: "var(--color-background-muted)",
    color: "var(--color-text-secondary)",
    border: "none",
    borderRadius: "8px 0 0 8px",
  },
  ".cm-activeLine, .cm-activeLineGutter": { backgroundColor: "var(--color-overlay-hover)" },
  ".cm-jinja": { color: "var(--color-syntax-variable)", fontWeight: "600" },
  ".cm-jinja-comment": { color: "var(--color-syntax-comment)" },
  ".cm-error-line": { backgroundColor: "var(--color-error-muted)" },
  ".cm-cursor": { borderLeftColor: "var(--color-text-primary)" },
});

/** What the React side drives: the document and the problem's line. */
export type EditorHandle = {
  setValue: (value: string) => void;
  setErrorLine: (line: number | null) => void;
  destroy: () => void;
};

export function createEditor({
  parent,
  doc,
  label,
  language,
  isReadOnly,
  minHeight,
  onChange,
}: {
  parent: HTMLElement;
  doc: string;
  label: string;
  language: "html" | "text";
  isReadOnly: boolean;
  minHeight: number;
  onChange: (value: string) => void;
}): EditorHandle {
  const extensions: Extension[] = [
    basicSetup,
    EditorView.lineWrapping,
    theme,
    syntaxHighlighting(highlight),
    jinja,
    errorLine,
    EditorView.contentAttributes.of({ "aria-label": label }),
    EditorView.editable.of(!isReadOnly),
    EditorView.theme({ ".cm-content, .cm-gutter": { minHeight: `${minHeight}px` } }),
    EditorView.updateListener.of((update) => {
      if (update.docChanged) onChange(update.state.doc.toString());
    }),
  ];
  if (language === "html") extensions.push(html());
  const view = new EditorView({ doc, extensions, parent });
  return {
    setValue(value) {
      const current = view.state.doc.toString();
      if (current !== value) {
        view.dispatch({ changes: { from: 0, to: current.length, insert: value } });
      }
    },
    setErrorLine(at) {
      const line = at !== null && at >= 1 && at <= view.state.doc.lines ? at : null;
      view.dispatch({
        effects: [
          setErrorLine.of(line),
          ...(line === null
            ? []
            : [EditorView.scrollIntoView(view.state.doc.line(line).from, { y: "center" })]),
        ],
      });
    },
    destroy: () => view.destroy(),
  };
}
