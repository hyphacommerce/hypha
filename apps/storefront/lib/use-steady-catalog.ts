import { useState } from "react";

import { sameCatalog } from "./catalog";

/** The catalog this component was first given, for as long as each new one is equal:
 * every server render of a layout (a navigation, an action's `refresh()`) sends an equal
 * catalog as a new object, and a provider keyed on it would rebuild what it makes from
 * it (a translator, a lookup) and re-render everything that reads one. */
export function useSteadyCatalog<C extends Readonly<Record<string, string>> | undefined>(
  catalog: C,
): C {
  const [kept, setKept] = useState(catalog);
  if (sameCatalog(kept, catalog)) return kept;
  // React's way to keep state in step with a prop: rendered again at once, before
  // anything is committed.
  setKept(catalog);
  return catalog;
}
