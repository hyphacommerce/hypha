"use client";

// The translator for client components: the shared namespaces every page is sent
// (`SHARED_NAMESPACES`), plus those a component names, which its route must have sent
// (`components/messages.tsx`: checkout and the account do). A key outside them doesn't
// compile, so a component can't read a message its page wasn't given.

import { useT as useAnyT } from "@hyphacommerce/i18n/react";

import type { Namespace, SharedNamespace, TFor } from "./messages";

export const useT = <N extends Namespace = never>(): TFor<SharedNamespace | N> => useAnyT();
