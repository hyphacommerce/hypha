"use client";

import { TextInput } from "@astryxdesign/core/TextInput";
import { MagnifyingGlassIcon } from "@heroicons/react/24/outline";
import { useEffect, useRef, useState } from "react";

/**
 * Searches the media library in a picker (design note 0024): file names, alt text and
 * titles. `onSearch` runs once the user pauses typing, and on Enter.
 */
export function LibrarySearch({ onSearch }: { onSearch: (q: string) => void }) {
  const [q, setQ] = useState("");
  const searched = useRef("");
  const search = (text: string) => {
    if (text.trim() === searched.current) return;
    searched.current = text.trim();
    onSearch(searched.current);
  };
  useEffect(() => {
    const timer = setTimeout(() => search(q), 400);
    return () => clearTimeout(timer);
  });
  return (
    <TextInput
      label="Search the library"
      isLabelHidden
      placeholder="Search names, alt text, titles"
      startIcon={MagnifyingGlassIcon}
      hasClear
      width="100%"
      value={q}
      onChange={setQ}
      onEnter={() => search(q)}
    />
  );
}
