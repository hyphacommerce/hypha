"use client";

import { TextInput } from "@astryxdesign/core/TextInput";
import { MagnifyingGlassIcon } from "@heroicons/react/24/outline";
import { useRouter } from "next/navigation";
import { useState } from "react";

/**
 * Searches the product list by title, handle or SKU (design note 0019, SE8): Enter
 * loads the list with `q`, keeping the status filter; clearing it shows them all again.
 */
export function SearchBox({ q: initial, status }: { q: string; status?: string }) {
  const router = useRouter();
  const [q, setQ] = useState(initial);
  const go = (text: string) => {
    const params = new URLSearchParams();
    if (status) params.set("status", status);
    if (text.trim()) params.set("q", text.trim());
    const search = params.toString();
    router.push(search ? `/products?${search}` : "/products");
  };
  return (
    <TextInput
      label="Search products"
      isLabelHidden
      placeholder="Search by title, handle or SKU"
      startIcon={<MagnifyingGlassIcon width={16} aria-hidden />}
      hasClear
      width={320}
      value={q}
      onChange={(text) => {
        setQ(text);
        // The clear button empties it: show everything again.
        if (text === "" && initial !== "") go("");
      }}
      onEnter={() => go(q)}
    />
  );
}
