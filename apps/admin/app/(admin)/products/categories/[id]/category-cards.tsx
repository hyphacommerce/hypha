"use client";

import { AlertDialog } from "@astryxdesign/core/AlertDialog";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { CheckboxInput } from "@astryxdesign/core/CheckboxInput";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { EmptyState } from "@astryxdesign/core/EmptyState";
import { Grid } from "@astryxdesign/core/Grid";
import { IconButton } from "@astryxdesign/core/IconButton";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { SelectableCard } from "@astryxdesign/core/SelectableCard";
import { Selector } from "@astryxdesign/core/Selector";
import { Spinner } from "@astryxdesign/core/Spinner";
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
import { PhotoIcon, TrashIcon } from "@heroicons/react/24/outline";
import { useEffect, useState, useTransition } from "react";

import { Pager } from "@/components/pager";
import { RichTextField } from "@/components/rich-text-field";
import { ProductStatusBadge } from "@/components/status";
import { type CategoryNode, VISIBILITY, subtree } from "@/lib/categories";

import { LibrarySearch } from "@/components/library-search";
import { libraryPage } from "../../actions";
import {
  addCategoryProducts,
  deleteCategory,
  findProducts,
  removeCategoryProduct,
  setCategoryImage,
  updateCategory,
} from "../actions";
import { ParentSelector, TOP_LEVEL } from "../category-tree";

type Category = AdminSchemas["CategoryResponse"];
type Media = AdminSchemas["ProductMediaResponse"];
type Asset = AdminSchemas["AssetResponse"];

/** Name, handle, description, parent and visibility: one Save sends what changed. */
export function CategoryForm({ category, tree }: { category: Category; tree: CategoryNode[] }) {
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
    <Card>
      <VStack gap={4}>
        <Text type="display-3" as="h2">
          Details
        </Text>
        {error && <Banner status="error" title={error} />}
        {isSaved && !isDirty && <Banner status="success" title="Saved." />}
        <HStack gap={4}>
          <TextInput label="Name" value={form.name} onChange={(name) => set({ name })} />
          <TextInput
            label="Handle"
            description="Its address in the storefront. Moving it keeps it."
            value={form.handle}
            onChange={(handle) => set({ handle })}
          />
        </HStack>
        <RichTextField
          label="Description"
          description="For the category page and search engines."
          isOptional
          value={form.description}
          onChange={(description) => set({ description })}
        />
        <HStack gap={4}>
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
        </HStack>
        <HStack gap={2} hAlign="end">
          {isDirty && <Button label="Discard" variant="ghost" onClick={() => setForm(saved)} />}
          <Button
            label="Save"
            variant="primary"
            isDisabled={!isDirty || form.name.trim() === "" || form.handle === ""}
            isLoading={isPending}
            onClick={save}
          />
        </HStack>
      </VStack>
    </Card>
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
              Shown on the category page and in menus. It can't be deleted from the library
              while it's here.
            </Text>
          </VStack>
          <HStack gap={2}>
            {image && (
              <Button label="Remove" variant="ghost" isLoading={isPending} onClick={() => set(null)} />
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
          // A plain <img>: the media URLs resize, so no Next.js image pipeline (0011 §2).
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

/** The library's images, newest first; pick one. */
function ImagePicker({
  isSaving,
  onPick,
  onClose,
}: {
  isSaving: boolean;
  onPick: (asset: string) => void;
  onClose: () => void;
}) {
  const [assets, setAssets] = useState<Asset[]>();
  const [next, setNext] = useState<string>();
  const [error, setError] = useState<string>();
  const [picked, setPicked] = useState<string>();
  const [isLoading, startLoading] = useTransition();

  const [query, setQuery] = useState("");
  const load = (cursor?: string, q = query) =>
    startLoading(async () => {
      const page = await libraryPage(cursor, q);
      if ("error" in page) {
        setError(page.error);
        return;
      }
      // A category shows an image: videos aren't offered.
      const images = page.items.filter((a) => a.kind === "image");
      setAssets((before) => [...(cursor ? (before ?? []) : []), ...images]);
      setNext(page.next);
    });
  // The first page, once, when the dialog opens.
  useEffect(() => load(), []);

  return (
    <Dialog isOpen onOpenChange={(isOpen) => !isOpen && onClose()} width={760} purpose="form">
      <DialogHeader
        title="Pick an image"
        subtitle="Images from the library, newest first."
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      <VStack padding={4} gap={0}>
        <LibrarySearch
          onSearch={(q) => {
            setQuery(q);
            load(undefined, q);
          }}
        />
      </VStack>
      <div style={{ flex: "1 1 auto", minHeight: 0, overflowY: "auto" }}>
        <VStack gap={4} padding={4}>
          {assets === undefined ? (
            <HStack hAlign="center" padding={6}>
              <Spinner aria-label="Loading the library" />
            </HStack>
          ) : assets.length === 0 ? (
            <EmptyState
              icon={<PhotoIcon width={32} />}
              title={query ? `No images match “${query}”` : "No images yet"}
              description={
                query
                  ? "Search looks at file names, alt text and titles."
                  : "Upload images on the Media page."
              }
            />
          ) : (
            <Grid columns={{ minWidth: 130 }} gap={2}>
              {assets.map((asset) => (
                <SelectableCard
                  key={asset.id}
                  label={asset.title ?? asset.filename}
                  isSelected={picked === asset.id}
                  isDisabled={asset.status !== "ready" || asset.visibility !== "public"}
                  onChange={(isSelected) => setPicked(isSelected ? asset.id : undefined)}
                  padding={0}
                >
                  <VStack gap={0}>
                    <img
                      src={asset.thumbnail_url ?? asset.url ?? ""}
                      alt=""
                      style={{ width: "100%", aspectRatio: "1", objectFit: "cover" }}
                    />
                    <VStack gap={0} padding={2}>
                      <Text type="supporting" maxLines={1}>
                        {asset.title ?? asset.filename}
                      </Text>
                    </VStack>
                  </VStack>
                </SelectableCard>
              ))}
            </Grid>
          )}
          {next && (
            <HStack hAlign="center">
              <Button label="Load more" variant="ghost" isLoading={isLoading} onClick={() => load(next)} />
            </HStack>
          )}
        </VStack>
      </div>
      <VStack gap={3} padding={4}>
        {error && <Banner status="error" title={error} />}
        <HStack gap={2} hAlign="end">
          <Button label="Cancel" variant="ghost" onClick={onClose} />
          <Button
            label="Use image"
            variant="primary"
            isDisabled={!picked}
            isLoading={isSaving}
            onClick={() => picked && onPick(picked)}
          />
        </HStack>
      </VStack>
    </Dialog>
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
      {isAdding && <AddProductsDialog id={id} onClose={() => setIsAdding(false)} />}
    </Card>
  );
}

/** Search products by title, handle or SKU, tick some, add them. */
function AddProductsDialog({ id, onClose }: { id: string; onClose: () => void }) {
  const [q, setQ] = useState("");
  const [found, setFound] = useState<{ id: string; title: string; handle: string }[]>();
  const [picked, setPicked] = useState<string[]>([]);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();

  const search = () => startTransition(async () => setFound(await findProducts(q)));
  const add = () =>
    startTransition(async () => {
      const result = await addCategoryProducts(id, picked);
      if (result.error) setError(result.error);
      else onClose();
    });

  return (
    <Dialog isOpen onOpenChange={(isOpen) => !isOpen && onClose()} width={560} purpose="form">
      <DialogHeader title="Add products" onOpenChange={(isOpen) => !isOpen && onClose()} />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <HStack gap={2} vAlign="end">
          <TextInput
            label="Search"
            placeholder="Title, handle or SKU"
            value={q}
            onChange={setQ}
            onEnter={search}
            hasAutoFocus
          />
          <Button label="Search" onClick={search} isLoading={isPending && !found} />
        </HStack>
        {found && found.length === 0 && <Text color="secondary">No products match.</Text>}
        {found?.map((product) => (
          <CheckboxInput
            key={product.id}
            label={product.title}
            description={`/${product.handle}`}
            value={picked.includes(product.id)}
            onChange={(checked) =>
              setPicked(checked ? [...picked, product.id] : picked.filter((p) => p !== product.id))
            }
          />
        ))}
        <HStack gap={2} hAlign="end">
          <Button label="Cancel" variant="ghost" onClick={onClose} />
          <Button
            label={picked.length > 1 ? `Add ${picked.length} products` : "Add product"}
            variant="primary"
            isDisabled={picked.length === 0}
            isLoading={isPending && found !== undefined}
            onClick={add}
          />
        </HStack>
      </VStack>
    </Dialog>
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
