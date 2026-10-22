const ORIGIN = "http://admin.invalid";

/** The pages outside the login, which make no sense to come back to after it. */
const PUBLIC_PAGES = [
  "/login",
  "/session-expired",
  "/invite",
  "/forgot-password",
  "/reset-password",
];

/**
 * A path on this admin to go to after signing in, or "/". Only plain local paths: never
 * another origin (`//evil.example`, `/\\evil.example`), nor the login pages themselves.
 */
export function safeNext(path: string | null | undefined): string {
  if (!path || !path.startsWith("/")) return "/";
  // Parsed as a browser would: it drops tabs and newlines, so "/\t/evil.example" is
  // "//evil.example", another origin.
  const url = new URL(path, ORIGIN);
  if (url.origin !== ORIGIN) return "/";
  path = url.pathname + url.search + url.hash;
  if (
    PUBLIC_PAGES.some(
      (page) => path === page || path.startsWith(`${page}/`) || path.startsWith(`${page}?`),
    )
  ) {
    return "/";
  }
  return path;
}
