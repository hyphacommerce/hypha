"use server";

// What the product picker (`product-picker.tsx`) reads: a page of products as the
// products list sees them, and one product's variants for picks that need a variant.

import type { AdminSchemas } from "@hypha/sdk/admin";

import { sessionClient } from "@/lib/hypha";

type Status = AdminSchemas["ProductStatus"];

export type PickerProduct = {
  id: string;
  title: string;
  handle: string;
  status: Status;
  variantCount: number;
  /** The featured image's thumbnail, if it has one. */
  image: string | null;
  /** The categories it's directly in, so a category's picker can mark them. */
  categories: string[];
};

export type PickerPage = { items: PickerProduct[]; nextCursor: string | null; error?: string };

/** A page of products, newest first, narrowed to text `q` and `statuses` when given. */
export async function pickerProducts(input: {
  q?: string;
  statuses?: Status[];
  cursor?: string;
}): Promise<PickerPage> {
  const client = await sessionClient();
  const { data, error } = await client.GET("/admin/products", {
    params: {
      query: {
        q: input.q?.trim() || undefined,
        status: input.statuses?.length ? input.statuses : undefined,
        cursor: input.cursor,
        limit: 20,
      },
    },
  });
  if (!data) {
    return { items: [], nextCursor: null, error: error?.error ?? "Couldn't load products." };
  }
  return {
    items: data.items.map((p) => ({
      id: p.id,
      title: p.title,
      handle: p.handle,
      status: p.status,
      variantCount: p.variant_count,
      image:
        p.featured_media?.kind === "image"
          ? (p.featured_media.thumbnail_url ?? p.featured_media.url)
          : null,
      categories: p.categories,
    })),
    nextCursor: data.next_cursor ?? null,
  };
}

export type PickerVariant = {
  id: string;
  /** "M / Blue"; null for a product without options. */
  title: string | null;
  sku: string | null;
  price: AdminSchemas["Money"];
};

/** A product's variants, in its order. */
export async function pickerVariants(product: string): Promise<PickerVariant[]> {
  const client = await sessionClient();
  const { data } = await client.GET("/admin/products/{id}", {
    params: { path: { id: product } },
  });
  return (data?.variants ?? []).map((v) => ({
    id: v.id,
    title: v.title ?? null,
    sku: v.sku ?? null,
    price: v.price,
  }));
}
