import type { Metadata } from "next";

import { getT } from "@/lib/i18n";

import { ForgotForm } from "./forgot-form";

export async function generateMetadata(): Promise<Metadata> {
  const t = await getT();
  return { title: t("auth.forgot_page_title") };
}

export default function ForgotPasswordPage() {
  return <ForgotForm />;
}
