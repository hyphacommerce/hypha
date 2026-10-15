// Where an expired or revoked session lands (`sessionClient` in lib/hypha.ts): pages
// can't change cookies while rendering, so this handler forgets the dead session,
// remembers the page to come back to, and sends the user to sign in again.

import { redirect } from "next/navigation";
import type { NextRequest } from "next/server";

import { endSession, rememberNext } from "@/lib/hypha";

export async function GET(request: NextRequest) {
  await endSession();
  await rememberNext(request.nextUrl.searchParams.get("next") ?? "/");
  redirect("/login");
}
