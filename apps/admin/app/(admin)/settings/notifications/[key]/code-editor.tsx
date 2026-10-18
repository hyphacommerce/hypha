"use client";

// A template source's editor (design note 0028, §7). CodeMirror (`codemirror-setup.ts`)
// loads once the editor mounts; until then the box shows empty.

import { useEffect, useEffectEvent, useRef } from "react";

import type { EditorHandle } from "./codemirror-setup";

export function CodeEditor({
  label,
  value,
  onChange,
  language,
  errorAt,
  isReadOnly = false,
  minHeight = 240,
}: {
  /** What a screen reader announces for the editor. */
  label: string;
  value: string;
  onChange: (value: string) => void;
  language: "html" | "text";
  /** The line (from 1) a problem is on, to highlight; `null` for none. */
  errorAt: number | null;
  isReadOnly?: boolean;
  minHeight?: number;
}) {
  const host = useRef<HTMLDivElement>(null);
  const editor = useRef<EditorHandle>(null);
  const changed = useEffectEvent((text: string) => onChange(text));
  // What it shows once loaded: later changes arrive through the effects below.
  const current = useEffectEvent(() => ({ value, errorAt }));

  // The editor, once: CodeMirror owns its DOM from here.
  useEffect(() => {
    let isCancelled = false;
    const mount = async () => {
      const { createEditor } = await import("./codemirror-setup");
      if (isCancelled || !host.current) return;
      const now = current();
      editor.current = createEditor({
        parent: host.current,
        doc: now.value,
        label,
        language,
        isReadOnly,
        minHeight,
        onChange: (text) => changed(text),
      });
      editor.current.setErrorLine(now.errorAt);
    };
    void mount();
    return () => {
      isCancelled = true;
      editor.current?.destroy();
      editor.current = null;
    };
  }, [label, language, isReadOnly, minHeight]);

  // A value set from outside (discard, revert): replace the document.
  useEffect(() => editor.current?.setValue(value), [value]);

  // The problem's line: marked, and scrolled into view.
  useEffect(() => editor.current?.setErrorLine(errorAt), [errorAt]);

  return <div ref={host} className="code-editor" style={{ minHeight }} />;
}
