"use client";

import { Button } from "@astryxdesign/core/Button";

import { AuthCard } from "./auth-card";

const SIGN_IN = { label: "Go to sign in", href: "/login" };

/** An emailed link that can't work as opened (no token): what to do instead. */
export function LinkProblem({
  title,
  subtitle,
  action = SIGN_IN,
}: {
  title: string;
  subtitle: string;
  action?: { label: string; href: string };
}) {
  return (
    <AuthCard title={title} subtitle={subtitle}>
      <Button label={action.label} variant="primary" size="lg" href={action.href} />
    </AuthCard>
  );
}
