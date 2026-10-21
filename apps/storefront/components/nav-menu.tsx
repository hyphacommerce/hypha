"use client";

// A header category with subcategories (components/site-header.tsx): the category's
// link, then a chevron button that discloses the subcategories in a panel under it.
// The panel opens on hover (after a moment, so passing over the bar opens nothing),
// on the button's click and when a key moves into it, and closes when the pointer
// leaves the group for a moment, on Escape, when focus leaves the group or on a click
// elsewhere. The header keeps one open at a time (`isOpen` / `onOpenChange`).

import { TopNavItem } from "@astryxdesign/core/TopNav";
import { ChevronDownIcon } from "@heroicons/react/16/solid";
import { usePathname } from "next/navigation";
import { useEffect, useEffectEvent, useId, useRef } from "react";

import { SiteLink } from "./site-link";

export type NavCategory = {
  id: string;
  name: string;
  href: string;
  children: { id: string; name: string; href: string }[];
};

/** Whether `pathname` is `href` or under it. */
export function within(pathname: string, href: string): boolean {
  return pathname === href || pathname.startsWith(`${href}/`);
}

/** The pointer must rest on the group this long before the panel opens. */
const OPEN_DELAY_MS = 120;
/** The pointer may leave the group this long (crossing to the panel) before it closes. */
const CLOSE_DELAY_MS = 200;

export function NavMenu({
  category,
  isOpen,
  onOpenChange,
}: {
  category: NavCategory;
  isOpen: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const pathname = usePathname();
  const panelId = useId();
  const group = useRef<HTMLDivElement>(null);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const later = (open: boolean, ms: number) => {
    clearTimeout(timer.current);
    timer.current = setTimeout(() => onOpenChange(open), ms);
  };
  useEffect(() => () => clearTimeout(timer.current), []);

  // A click or tap outside closes it (a touch screen opens with the button).
  const onPointerDown = useEffectEvent((event: PointerEvent) => {
    if (event.target instanceof Node && !group.current?.contains(event.target)) {
      onOpenChange(false);
    }
  });
  useEffect(() => {
    if (!isOpen) return;
    document.addEventListener("pointerdown", onPointerDown);
    return () => document.removeEventListener("pointerdown", onPointerDown);
  }, [isOpen]);

  const hasChildren = category.children.length > 0;
  return (
    <div
      ref={group}
      className="shop-nav-group"
      role="presentation"
      data-open={isOpen || undefined}
      onPointerEnter={(event) => {
        if (hasChildren && event.pointerType === "mouse") later(true, OPEN_DELAY_MS);
      }}
      onPointerLeave={(event) => {
        if (event.pointerType === "mouse") later(false, CLOSE_DELAY_MS);
      }}
      onKeyDown={(event) => {
        if (event.key === "Escape" && isOpen) {
          event.stopPropagation();
          onOpenChange(false);
          group.current?.querySelector("button")?.focus();
        }
      }}
      onBlur={(event) => {
        // Focus moved out of the group (Tab past the last subcategory).
        if (!group.current?.contains(event.relatedTarget)) {
          clearTimeout(timer.current);
          onOpenChange(false);
        }
      }}
    >
      <div className="shop-nav-trigger">
        <TopNavItem
          label={category.name}
          href={category.href}
          isSelected={within(pathname, category.href)}
        />
        {hasChildren && (
          <button
            type="button"
            className="shop-nav-chevron"
            aria-label={`${category.name} subcategories`}
            aria-expanded={isOpen}
            aria-controls={panelId}
            onClick={() => {
              clearTimeout(timer.current);
              onOpenChange(!isOpen);
            }}
          >
            <ChevronDownIcon aria-hidden="true" width={16} height={16} />
          </button>
        )}
      </div>
      {hasChildren && (
        <div id={panelId} className="shop-nav-sub" hidden={!isOpen}>
          {category.children.map((child) => (
            <SiteLink
              key={child.id}
              href={child.href}
              className="shop-nav-sub-link"
              aria-current={within(pathname, child.href) ? "page" : undefined}
            >
              {child.name}
            </SiteLink>
          ))}
        </div>
      )}
    </div>
  );
}
