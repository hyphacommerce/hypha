"use client";

// Line 1 of an address form with the store's autocomplete: what the shopper types stays
// the line's text (a field they can always finish by hand), and after a pause the store's
// autocomplete app offers addresses under it, as buttons. Picking one fills the form.
// Astryx's Typeahead keeps its query to itself and shows a pick as a token, so this is
// its TextInput with a list of its own: the down arrow goes from the field to the
// suggestions, the arrows move between them, Escape goes back to the field.

import { TextInput } from "@astryxdesign/core/TextInput";
import { useEffect, useId, useRef, useState } from "react";

import type { AddressSuggestion } from "@/app/[country]/actions";

/** How long typing pauses before asking: a lookup per word, not per keystroke. */
const DEBOUNCE_MS = 250;

/** The fewest characters worth a lookup (the API's minimum). */
const MIN_CHARS = 3;

/** A new token for one address being typed: providers bill per session. */
function newSession(): string {
  return crypto.randomUUID().replaceAll("-", "");
}

export function AddressLineInput({
  label,
  value,
  onChange,
  onPick,
  suggest,
  isRequired,
  status,
  size,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  /** A suggestion was picked, in `session`; the form resolves and fills it. */
  onPick: (suggestion: AddressSuggestion, session: string) => Promise<void>;
  suggest: (q: string, session: string) => Promise<AddressSuggestion[]>;
  isRequired: boolean;
  status?: { type: "error" | "warning" | "success"; message?: string };
  size?: "lg";
}) {
  const listId = useId();
  const [session, setSession] = useState(newSession);
  // What the shopper typed last, apart from changes the form makes (a pick, a reset).
  const [typed, setTyped] = useState<string | null>(null);
  const [suggestions, setSuggestions] = useState<AddressSuggestion[]>([]);
  const [open, setOpen] = useState(false);
  const [resolving, setResolving] = useState(false);
  // The latest lookup: an older one answering late is dropped.
  const asked = useRef(0);
  const wrapper = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const q = typed?.trim() ?? "";
    if (q.length < MIN_CHARS) return;
    const ask = ++asked.current;
    const timer = setTimeout(async () => {
      // No suggestions when the lookup fails: the shopper types the address.
      const found = await suggest(q, session).catch(() => []);
      if (ask !== asked.current) return;
      setSuggestions(found);
      // An answer arriving after focus left the field doesn't open the list under it.
      setOpen(found.length > 0 && !!wrapper.current?.contains(document.activeElement));
    }, DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [typed, session, suggest]);

  const options = () => [
    ...(wrapper.current?.querySelectorAll<HTMLButtonElement>(".shop-address-options button") ?? []),
  ];
  const fieldInput = () => wrapper.current?.querySelector<HTMLInputElement>("input");

  const pick = async (suggestion: AddressSuggestion) => {
    asked.current++;
    setOpen(false);
    setSuggestions([]);
    setTyped(null);
    fieldInput()?.focus();
    setResolving(true);
    try {
      await onPick(suggestion, session);
    } finally {
      setResolving(false);
      // The next address typed is a new session.
      setSession(newSession());
    }
  };

  const shown = open && suggestions.length > 0;
  // Arrows from the field into the list and along it; Escape back to the field.
  const onKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    if (!shown) return;
    const buttons = options();
    const at = buttons.findIndex((b) => b === document.activeElement);
    if (e.key === "ArrowDown") {
      e.preventDefault();
      buttons[(at + 1) % buttons.length]?.focus();
    } else if (e.key === "ArrowUp" && at >= 0) {
      e.preventDefault();
      if (at === 0) fieldInput()?.focus();
      else buttons[at - 1]?.focus();
    } else if (e.key === "Escape") {
      e.preventDefault();
      setOpen(false);
      fieldInput()?.focus();
    }
  };

  return (
    // oxlint-disable-next-line jsx-a11y/no-static-element-interactions -- the keys move focus between the field and the suggestions inside it
    <div
      ref={wrapper}
      className="shop-address-line"
      onKeyDown={onKeyDown}
      onBlur={(e) => {
        // Closed once the focus leaves the field and its suggestions.
        if (!e.currentTarget.contains(e.relatedTarget)) setOpen(false);
      }}
    >
      <TextInput
        size={size}
        label={label}
        isRequired={isRequired}
        isOptional={!isRequired}
        value={value}
        onChange={(next) => {
          onChange(next);
          setTyped(next);
          if (next.trim().length < MIN_CHARS) {
            asked.current++;
            setSuggestions([]);
          }
        }}
        isLoading={resolving}
        autoComplete="address-line1"
        status={status}
        aria-controls={listId}
        aria-expanded={shown}
      />
      {shown && (
        <div id={listId} className="shop-address-options">
          {suggestions.map((s) => (
            <button key={s.id} type="button" onClick={() => void pick(s)}>
              <span className="shop-address-option-label">{s.label}</span>
              {s.description && (
                <span className="shop-address-option-description">{s.description}</span>
              )}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
