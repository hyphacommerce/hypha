// Server-only: the names of the resources on a page of the translation worklist. Never
// import this from a "use client" file.

import { pathsById } from "./categories";
import { sessionClient } from "./hypha";
import type { T } from "./messages";
import {
  NAMED_KINDS,
  type NamedKind,
  type WorklistItem,
  idsToName,
  nameKey,
} from "./translation-worklist";

type Client = Awaited<ReturnType<typeof sessionClient>>;
type Names = [kind: NamedKind, id: string, name: string][];

/**
 * The names of a page's resources, by `nameKey`: one read per kind on the page, products
 * by their IDs and the kinds the API lists whole. A read that fails (a role that can read
 * a kind's translations but not, say, its zones) names nothing of its kind, and the
 * table shows those by kind and ID.
 */
export async function resourceNames(
  items: readonly WorklistItem[],
  t: T,
): Promise<Record<string, string>> {
  const ids = idsToName(items);
  const client = await sessionClient();
  const reads = NAMED_KINDS.flatMap((kind) => {
    const wanted = ids[kind];
    return wanted ? [namesOf(client, kind, wanted, t).catch((): Names => [])] : [];
  });
  const found = (await Promise.all(reads)).flat();
  return Object.fromEntries(found.map(([kind, id, name]) => [nameKey(kind, id), name]));
}

async function namesOf(client: Client, kind: NamedKind, ids: string[], t: T): Promise<Names> {
  const wanted = new Set(ids);
  const named = (pairs: [string, string][]): Names =>
    pairs.flatMap(([id, name]) => (wanted.has(id) && name.trim() !== "" ? [[kind, id, name]] : []));
  switch (kind) {
    case "product": {
      const { data } = await client.GET("/admin/products", {
        params: { query: { ids, limit: Math.min(100, ids.length) } },
      });
      return named((data?.items ?? []).map((p) => [p.id, p.title]));
    }
    case "category": {
      const { data } = await client.GET("/admin/categories");
      return named([...pathsById(data?.items ?? [])]);
    }
    case "custom_field_definition": {
      const { data } = await client.GET("/admin/custom-field-definitions");
      return named((data?.items ?? []).map((d) => [d.id, d.name]));
    }
    case "shipping_option": {
      // Zones often share option names ("Standard"): the zone says which one it is.
      const { data } = await client.GET("/admin/shipping-zones");
      return named(
        (data?.items ?? []).flatMap((zone) =>
          zone.options.map((option): [string, string] => [
            option.id,
            t("translations.worklist_shipping_option", { option: option.name, zone: zone.name }),
          ]),
        ),
      );
    }
    case "payment_method": {
      const { data } = await client.GET("/admin/payment-methods");
      return named((data?.items ?? []).map((m) => [m.id, m.name]));
    }
    case "checkout_rule": {
      const { data } = await client.GET("/admin/checkout-rules");
      return named((data?.items ?? []).map((r) => [r.id, r.name]));
    }
  }
}
