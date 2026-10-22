// The account pages' frame: the heading, the section links and the page.
// Server-rendered; the links are the site's.

import { Heading } from "@astryxdesign/core/Heading";
import { HStack, VStack } from "@astryxdesign/core/Stack";

import { SiteLink } from "@/components/site-link";

export const SECTIONS = [
  { path: "/account", label: "Overview" },
  { path: "/account/orders", label: "Orders" },
  { path: "/account/addresses", label: "Addresses" },
  { path: "/account/profile", label: "Profile" },
  { path: "/account/marketing", label: "Marketing" },
  { path: "/account/privacy", label: "Privacy" },
] as const;

export type Section = (typeof SECTIONS)[number]["path"];

export function AccountShell({
  title,
  current,
  link,
  children,
}: {
  title: string;
  current: Section;
  /** `path` → the page's URL in the shopper's country. */
  link: (path: string) => string;
  children: React.ReactNode;
}) {
  return (
    <VStack gap={6}>
      <Heading level={1} type="display-2">
        {title}
      </Heading>
      <nav aria-label="Account" className="shop-account-nav">
        <HStack gap={4} wrap="wrap">
          {SECTIONS.map((s) => (
            <SiteLink
              key={s.path}
              href={link(s.path)}
              aria-current={s.path === current ? "page" : undefined}
            >
              {s.label}
            </SiteLink>
          ))}
        </HStack>
      </nav>
      {children}
    </VStack>
  );
}
