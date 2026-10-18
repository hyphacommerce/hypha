import type { Metadata } from "next";

import { ForgotForm } from "./forgot-form";

export const metadata: Metadata = { title: "Reset your password · Hypha admin" };

export default function ForgotPasswordPage() {
  return <ForgotForm />;
}
