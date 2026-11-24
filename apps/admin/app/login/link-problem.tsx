"use client";

import { Button } from "@astryxdesign/core/Button";

import { useT } from "@/lib/use-t";

import { AuthCard } from "./auth-card";

/**
 * An emailed link that can't work as opened (no token): what to do instead. Without an
 * `action`, the way to sign in.
 */
export function LinkProblem({
  title,
  subtitle,
  action,
}: {
  title: string;
  subtitle: string;
  action?: { label: string; href: string };
}) {
  const t = useT<"auth">();
  const { label, href } = action ?? { label: t("auth.go_to_sign_in"), href: "/login" };
  return (
    <AuthCard title={title} subtitle={subtitle}>
      <Button label={label} variant="primary" size="lg" href={href} />
    </AuthCard>
  );
}
