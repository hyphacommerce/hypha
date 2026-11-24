// The account pages' frame: the heading, the section links and the page.
// Server-rendered; the links are the site's.

import { Heading } from "@astryxdesign/core/Heading";
import { HStack, VStack } from "@astryxdesign/core/Stack";

import { SiteLink } from "@/components/site-link";
import type { KeyIn, T } from "@/lib/messages";

/** The account's sections, each with its label's message. */
export const SECTIONS = [
  { path: "/account", label: "account.section_overview" },
  { path: "/account/orders", label: "account.section_orders" },
  { path: "/account/addresses", label: "account.section_addresses" },
  { path: "/account/profile", label: "account.section_profile" },
  { path: "/account/marketing", label: "account.section_marketing" },
  { path: "/account/privacy", label: "account.section_privacy" },
] as const satisfies readonly { path: string; label: KeyIn<"account"> }[];

export type Section = (typeof SECTIONS)[number]["path"];

export function AccountShell({
  t,
  title,
  current,
  link,
  children,
}: {
  t: T;
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
      <nav aria-label={t("account.nav_label")} className="shop-account-nav">
        <HStack gap={4} wrap="wrap">
          {SECTIONS.map((s) => (
            <SiteLink
              key={s.path}
              href={link(s.path)}
              aria-current={s.path === current ? "page" : undefined}
            >
              {t(s.label)}
            </SiteLink>
          ))}
        </HStack>
      </nav>
      {children}
    </VStack>
  );
}
