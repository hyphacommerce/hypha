"use client";

// The translator for client components: the shared namespaces (`SHARED_NAMESPACES`: the
// vocabulary, the frame, the shared components) plus those a component names.
//
//   const t = useT();              // common.*, components.*, nav.*, shell.*, table.*
//   const t = useT<"orders">();    // those and orders.*
//
// A key outside them doesn't compile, so a component says which catalogs it reads. The
// root layout's provider (`components/i18n-provider.tsx`) holds the translator. Server
// components and actions call `getT()` (`lib/i18n.ts`) instead.

import { useLanguage, useT as useAnyT } from "@hyphacommerce/i18n/react";

import type { Namespace, SharedNamespace, TFor } from "./messages";

export { useLanguage };

export const useT = <N extends Namespace = never>(): TFor<SharedNamespace | N> => useAnyT();
