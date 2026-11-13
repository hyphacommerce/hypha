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
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { TrashIcon } from "@heroicons/react/24/outline";
import { type ReactNode, useState, useTransition } from "react";

import { ImagePicker } from "@/components/image-picker";
import { Pager } from "@/components/pager";
import { ProductPicker } from "@/components/product-picker";
import { RichTextField } from "@/components/rich-text-field";
import { SaveBar } from "@/components/save-bar";
import { ProductStatusBadge } from "@/components/status";
import { type CategoryNode, subtree, visibilityOptions } from "@/lib/categories";
import { useT } from "@/lib/use-t";

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
  const t = useT<"categories">();
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
      {isSaved && !isDirty && <Banner status="success" title={t("categories.saved")} />}
      <div className="detail-columns">
        <div>
          <Card>
            <VStack gap={4}>
              <Text type="display-3" as="h2">
                {t("common.details")}
              </Text>
              <div className="form-grid">
                <TextInput
                  label={t("common.name")}
                  description={t("categories.name_description")}
                  value={form.name}
                  onChange={(name) => set({ name })}
                />
                <TextInput
                  label={t("common.handle")}
                  description={t("categories.handle_description")}
                  value={form.handle}
                  onChange={(handle) => set({ handle })}
                />
              </div>
              <RichTextField
                label={t("common.description")}
                description={t("categories.description_hint")}
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
                {t("categories.placement")}
              </Text>
              <ParentSelector
                tree={tree}
                value={form.parent}
                onChange={(parent) => set({ parent })}
                exclude={subtree(tree, category.id)}
              />
              <Selector
                label={t("categories.visibility")}
                description={t("categories.visibility_description")}
                options={visibilityOptions(t)}
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
  const t = useT<"categories">();
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
              {t("categories.image_title")}
            </Text>
            <Text type="supporting" color="secondary">
              {t("categories.image_hint")}
            </Text>
          </VStack>
          <HStack gap={2}>
            {image && (
              <Button
                label={t("common.remove")}
                variant="ghost"
                isLoading={isPending}
                onClick={() => set(null)}
              />
            )}
            <Button
              label={image ? t("common.change") : t("categories.image_pick")}
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
          <Text color="secondary">{t("categories.image_none")}</Text>
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
  const t = useT<"categories">();
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
              {t("common.products")}
            </Text>
            <Text type="supporting" color="secondary">
              {t("categories.products_hint")}
            </Text>
          </VStack>
          <Button
            label={t("categories.products_add")}
            variant="secondary"
            onClick={() => setIsAdding(true)}
          />
        </HStack>
        {error && <Banner status="error" title={error} />}
        {page.items.length === 0 ? (
          <Text color="secondary">
            {isFirstPage ? t("categories.products_none") : t("categories.products_no_more")}
          </Text>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHeaderCell>{t("common.product")}</TableHeaderCell>
                <TableHeaderCell>{t("common.status")}</TableHeaderCell>
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
                      <Text color="secondary">
                        {t("categories.product_missing", { id: product.id })}
                      </Text>
                    )}
                  </TableCell>
                  <TableCell>
                    {product.status && <ProductStatusBadge status={product.status} t={t} />}
                  </TableCell>
                  <TableCell>
                    <HStack hAlign="end">
                      <IconButton
                        label={t("categories.product_remove", { name: product.title ?? product.id })}
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
          t={t}
        />
      </VStack>
      {isAdding && (
        <ProductPicker
          title={t("categories.products_add")}
          description={t("categories.picker_description")}
          isAlreadyIn={(product) => product.categories.includes(id)}
          alreadyInLabel={t("categories.picker_already_in")}
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
  const t = useT<"categories">();
  const [isOpen, setIsOpen] = useState(false);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();

  return (
    <>
      <Button label={t("common.delete")} variant="ghost" onClick={() => setIsOpen(true)} />
      {error && <Banner status="error" title={error} />}
      <AlertDialog
        isOpen={isOpen}
        onOpenChange={setIsOpen}
        title={t("common.delete_named", { name })}
        description={t("categories.delete_description")}
        actionLabel={t("categories.delete_action")}
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
