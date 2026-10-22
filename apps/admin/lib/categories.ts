// The category tree as the admin's screens use it: the API's nested tree flattened
// into rows, with the paths pickers show. Shared by server and client components.

import type { AdminSchemas } from "@hypha/sdk/admin";

export type CategoryNode = AdminSchemas["CategoryNode"];

/** A category in tree order, with where it sits. */
export type CategoryRow = {
  id: string;
  name: string;
  handle: string;
  visible: boolean;
  position: number;
  /** 0 at the top level. */
  depth: number;
  /** "Clothing › Shirts". */
  path: string;
  parent: string | null;
  /** How many siblings it has, itself included (for move up/down). */
  siblings: number;
  /** Whether it, or an ancestor, is hidden: the storefront doesn't show it. */
  isHiddenInStore: boolean;
};

/** Every category, depth first in order. */
export function flatten(nodes: CategoryNode[]): CategoryRow[] {
  const rows: CategoryRow[] = [];
  const walk = (
    list: CategoryNode[],
    depth: number,
    parent: string | null,
    path: string,
    hidden: boolean,
  ) => {
    for (const node of list) {
      const own = path ? `${path} › ${node.name}` : node.name;
      const isHiddenInStore = hidden || !node.visible;
      rows.push({
        id: node.id,
        name: node.name,
        handle: node.handle,
        visible: node.visible,
        position: node.position,
        depth,
        path: own,
        parent,
        siblings: list.length,
        isHiddenInStore,
      });
      walk(node.children, depth + 1, node.id, own, isHiddenInStore);
    }
  };
  walk(nodes, 0, null, "", false);
  return rows;
}

/** `id` and every category below it: what it can't move under. */
export function subtree(nodes: CategoryNode[], id: string): Set<string> {
  const found = new Set<string>();
  const collect = (node: CategoryNode) => {
    found.add(node.id);
    node.children.forEach(collect);
  };
  const find = (list: CategoryNode[]): boolean =>
    list.some((node) => {
      if (node.id === id) {
        collect(node);
        return true;
      }
      return find(node.children);
    });
  find(nodes);
  return found;
}

/** Category ID → path, for labelling IDs (a product's categories). */
export function pathsById(nodes: CategoryNode[]): Map<string, string> {
  return new Map(flatten(nodes).map((row) => [row.id, row.path]));
}

/** A handle from a name, by the API's rule: lowercase letters, digits, single dashes. */
export function toHandle(name: string): string {
  return name
    .normalize("NFKD")
    .replace(/[̀-ͯ]/g, "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

/** The two visibility choices, Hidden first: a new category starts hidden unless the
 * merchant says otherwise, and the request always states which. */
export const VISIBILITY = [
  { value: "hidden", label: "Hidden", description: "The storefront doesn't show it yet." },
  { value: "visible", label: "Visible", description: "Shown in the storefront." },
] as const;
