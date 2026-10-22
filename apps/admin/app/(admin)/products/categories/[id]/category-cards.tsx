"use client";

import { AlertDialog } from "@astryxdesign/core/AlertDialog";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { IconButton } from "@astryxdesign/core/IconButton";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { Selector } from "@astryxdesign/core/Selector";
import {
  Table,
  TableBody,
  TableCell,
  TableHeader,
  TableHeaderCell,
  TableRow,
} from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import type { AdminSchemas } from "@hypha/sdk/admin";
import { TrashIcon } from "@heroicons/react/24/outline";
import { type ReactNode, useState, useTransition } from "react";

import { ImagePicker } from "@/components/image-picker";
import { Pager } from "@/components/pager";
import { ProductPicker } from "@/components/product-picker";
import { RichTextField } from "@/components/rich-text-field";
import { SaveBar } from "@/components/save-bar";
import { ProductStatusBadge } from "@/components/status";
import { type CategoryNode, VISIBILITY, subtree } from "@/lib/categories";

import {
  addCategoryProducts,
  deleteCategory,
  removeCategoryProduct,
  setCategoryImage,
  updateCategory,
} from "../actions";
import { ParentSelector, TOP_LEVEL } from "../category-tree";

type Category = AdminSchemas["CategoryResponse"];
type Media = AdminSchemas["ProductMediaResponse"];

/** Name, handle, description, parent and visibility: one Save sends what changed. */
/**
 * A category's page body: its details and products on the left, where it sits and its
 * image on the right. The name, handle, description, parent and visibility are one
 * draft saved by the bar at the top; the image and products save as they change (`main`
 * and `side`).
 */
export function CategoryForm({
  category,
  tree,
  main,
  side,
}: {
  category: Category;
  tree: CategoryNode[];
  main: ReactNode;
  side: ReactNode;
}) {
  const saved = {
    name: category.name,
    handle: category.handle,
    description: category.description ?? "",
    parent: category.parent ?? TOP_LEVEL,
    visibility: category.visible ? "visible" : "hidden",
  };
  const [form, setForm] = useState(saved);
  const [error, setError] = useState<string>();
  const [isSaved, setIsSaved] = useState(false);
  const [isPending, startTransition] = useTransition();
  const set = (change: Partial<typeof form>) => {
    setForm((before) => ({ ...before, ...change }));
    setIsSaved(false);
  };

  const body: AdminSchemas["UpdateCategoryRequest"] = {};
  if (form.name.trim() !== saved.name) body.name = form.name;
  if (form.handle !== saved.handle) body.handle = form.handle;
  if (form.description.trim() !== saved.description.trim()) {
    body.description = form.description.trim() === "" ? null : form.description;
  }
  if (form.parent !== saved.parent) body.parent = form.parent === TOP_LEVEL ? null : form.parent;
  if (form.visibility !== saved.visibility) body.visible = form.visibility === "visible";
  const isDirty = Object.keys(body).length > 0;

  const save = () =>
    startTransition(async () => {
      const result = await updateCategory(category.id, body);
      setError(result.error);
      setIsSaved(!result.error);
    });

  return (
    <>
      <SaveBar
        isDirty={isDirty}
        isPending={isPending}
        error={error}
        canSave={form.name.trim() !== "" && form.handle !== ""}
        onSave={save}
        onDiscard={() => {
          setForm(saved);
          setError(undefined);
        }}
      />
      {isSaved && !isDirty && <Banner status="success" title="Saved." />}
      <div className="detail-columns">
        <div>
          <Card>
            <VStack gap={4}>
              <Text type="display-3" as="h2">
                Details
              </Text>
              <div className="form-grid">
                <TextInput
                  label="Name"
                  description="Shown to shoppers in menus and on its page."
                  value={form.name}
                  onChange={(name) => set({ name })}
                />
                <TextInput
                  label="Handle"
                  description="Its address in the storefront. Moving it keeps it."
                  value={form.handle}
                  onChange={(handle) => set({ handle })}
                />
              </div>
              <RichTextField
                label="Description"
                description="For the category page and search engines."
                isOptional
                value={form.description}
                onChange={(description) => set({ description })}
              />
            </VStack>
          </Card>
          {main}
        </div>
        <div>
          <Card>
            <VStack gap={4}>
              <Text type="display-3" as="h2">
                Placement
              </Text>
              <ParentSelector
                tree={tree}
                value={form.parent}
                onChange={(parent) => set({ parent })}
                exclude={subtree(tree, category.id)}
              />
              <Selector
                label="Visibility"
                description="Hiding it hides everything below it too."
                options={[...VISIBILITY]}
                value={form.visibility}
                onChange={(visibility) => set({ visibility })}
              />
            </VStack>
          </Card>
          {side}
        </div>
      </div>
    </>
  );
}

/** The category's image, picked from the media library; in use while it's set. */
export function ImageCard({ id, image }: { id: string; image: Media | null }) {
  const [isPicking, setIsPicking] = useState(false);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const set = (asset: string | null) =>
    startTransition(async () => {
      const result = await setCategoryImage(id, asset);
      setError(result.error);
      if (!result.error) setIsPicking(false);
    });
  const src = image ? (image.thumbnail_url ?? image.url) : undefined;

  return (
    <Card>
      <VStack gap={3}>
        <HStack gap={2} hAlign="between" vAlign="center">
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              Image
            </Text>
            <Text type="supporting" color="secondary">
              Shown on the category page and in menus. It can't be deleted from the library while
              it's here.
            </Text>
          </VStack>
          <HStack gap={2}>
            {image && (
              <Button
                label="Remove"
                variant="ghost"
                isLoading={isPending}
                onClick={() => set(null)}
              />
            )}
            <Button
              label={image ? "Change" : "Pick from library"}
              variant="secondary"
              onClick={() => setIsPicking(true)}
            />
          </HStack>
        </HStack>
        {error && <Banner status="error" title={error} />}
        {src ? (
          // A plain <img>: the media URLs resize, so no Next.js image pipeline.
          <img
            src={src}
            alt={image?.alt ?? ""}
            style={{ maxWidth: 240, borderRadius: 8, objectFit: "cover" }}
          />
        ) : (
          <Text color="secondary">No image.</Text>
        )}
      </VStack>
      {isPicking && (
        <ImagePicker isSaving={isPending} onPick={set} onClose={() => setIsPicking(false)} />
      )}
    </Card>
  );
}

/** The products directly in the category, newest first. */
export function ProductsCard({
  id,
  page,
  isFirstPage,
}: {
  id: string;
  page: AdminSchemas["CategoryProductPage"];
  isFirstPage: boolean;
}) {
  const [error, setError] = useState<string>();
  const [isAdding, setIsAdding] = useState(false);
  const [isPending, startTransition] = useTransition();
  const remove = (product: string) =>
    startTransition(async () => setError((await removeCategoryProduct(id, product)).error));

  return (
    <Card>
      <VStack gap={3}>
        <HStack gap={2} hAlign="between" vAlign="center" wrap="wrap">
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              Products
            </Text>
            <Text type="supporting" color="secondary">
              The ones directly in it. The storefront lists its subcategories' products too.
            </Text>
          </VStack>
          <Button label="Add products" variant="secondary" onClick={() => setIsAdding(true)} />
        </HStack>
        {error && <Banner status="error" title={error} />}
        {page.items.length === 0 ? (
          <Text color="secondary">{isFirstPage ? "No products yet." : "No more products."}</Text>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHeaderCell>Product</TableHeaderCell>
                <TableHeaderCell>Status</TableHeaderCell>
                <TableHeaderCell>
                  <span />
                </TableHeaderCell>
              </TableRow>
            </TableHeader>
            <TableBody>
              {page.items.map((product) => (
                <TableRow key={product.id}>
                  <TableCell>
                    {product.title ? (
                      <VStack gap={0}>
                        <Link href={`/products/${product.id}`} weight="medium">
                          {product.title}
                        </Link>
                        <Text type="supporting" color="secondary">
                          /{product.handle}
                        </Text>
                      </VStack>
                    ) : (
                      // An ID stored as given that names no product.
                      <Text color="secondary">{product.id} (no such product)</Text>
                    )}
                  </TableCell>
                  <TableCell>
                    {product.status && <ProductStatusBadge status={product.status} />}
                  </TableCell>
                  <TableCell>
                    <HStack hAlign="end">
                      <IconButton
                        label={`Remove ${product.title ?? product.id}`}
                        icon={<TrashIcon />}
                        size="sm"
                        variant="ghost"
                        isDisabled={isPending}
                        onClick={() => remove(product.id)}
                      />
                    </HStack>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}
        <Pager
          path={`/products/categories/${id}`}
          query={{}}
          isFirstPage={isFirstPage}
          nextCursor={page.next_cursor}
        />
      </VStack>
      {isAdding && (
        <ProductPicker
          title="Add products"
          description="They're listed directly in this category."
          isAlreadyIn={(product) => product.categories.includes(id)}
          alreadyInLabel="Already in this category"
          onConfirm={async (products) => {
            const result = await addCategoryProducts(
              id,
              products.map((p) => p.id),
            );
            if (result.error) return result;
            setIsAdding(false);
          }}
          onClose={() => setIsAdding(false)}
        />
      )}
    </Card>
  );
}

/** Delete, confirmed in an alert dialog: the category and its membership go. */
export function DeleteCategory({ id, name }: { id: string; name: string }) {
  const [isOpen, setIsOpen] = useState(false);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();

  return (
    <>
      <Button label="Delete" variant="ghost" onClick={() => setIsOpen(true)} />
      {error && <Banner status="error" title={error} />}
      <AlertDialog
        isOpen={isOpen}
        onOpenChange={setIsOpen}
        title={`Delete ${name}?`}
        description="Its products stay; they just leave this category. A category with subcategories can't be deleted until they're moved or deleted."
        actionLabel="Delete category"
        isActionLoading={isPending}
        onAction={() =>
          startTransition(async () => {
            const result = await deleteCategory(id);
            if (result.error) {
              setError(result.error);
              setIsOpen(false);
            }
          })
        }
      />
    </>
  );
}
