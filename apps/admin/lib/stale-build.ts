// A page loaded before a deploy runs the old build's JavaScript, and asks for code split
// out of it (a section opened later, a dialog) under the old build's file names, which
// the deploy replaced: the import fails and the page would crash. Loading the page again
// gets the new build. Browser-only (it reloads), used by the error boundaries and
// `Providers`.

/** Each browser's words for a code file that couldn't be loaded. */
const STALE = [
  "Failed to fetch dynamically imported module", // Chrome
  "error loading dynamically imported module", // Firefox
  "Importing a module script failed", // Safari
  "Unable to preload CSS",
];

/** Whether `error` is a code file of the build that couldn't be loaded. */
export function isStaleBuild(error: unknown): boolean {
  const message = error instanceof Error ? error.message : String(error);
  return STALE.some((words) => message.includes(words));
}

/** How long one reload stands for: a second failure within it is a real outage, shown. */
const WINDOW_MS = 10_000;
const KEY = "hypha:stale-build-reload";

/**
 * Loads the page again, unless it was just loaded again for this (then the files are
 * missing for another reason, and reloading would loop). Whether it reloads.
 */
export function reloadForNewBuild(): boolean {
  let last = 0;
  try {
    last = Number(sessionStorage.getItem(KEY) ?? 0);
    sessionStorage.setItem(KEY, String(Date.now()));
  } catch {
    // No storage (a private window): reload anyway; a loop is cut by the browser.
  }
  if (Date.now() - last < WINDOW_MS) return false;
  window.location.reload();
  return true;
}
