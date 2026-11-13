"use client";

import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { EmptyState } from "@astryxdesign/core/EmptyState";
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
import { ArrowDownIcon, ArrowUpIcon, FolderIcon } from "@heroicons/react/24/outline";
import { useState, useTransition } from "react";

import {
  type CategoryNode,
  VISIBILITY,
  flatten,
  toHandle,
  visibilityOptions,
} from "@/lib/categories";
import { useT } from "@/lib/use-t";

import { createCategory, updateCategory } from "./actions";

/** The value of "Top level" in a parent picker: never a category ID (`pcat_…`). */
export const TOP_LEVEL = "top";

/** The tree as indented rows, each movable among its siblings. */
export function CategoryTree({ tree }: { tree: CategoryNode[] }) {
  const t = useT<"categories">();
  const rows = flatten(tree);
  const [isAdding, setIsAdding] = useState(false);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const move = (id: string, position: number) =>
    startTransition(async () => setError((await updateCategory(id, { position })).error));

  return (
    <Card>
      <VStack gap={3}>
        <HStack gap={2} hAlign="between" vAlign="center">
          <Text type="display-3" as="h2">
            {t("categories.tree_title")}
          </Text>
          <Button
            label={t("categories.add")}
            variant="primary"
            onClick={() => setIsAdding(true)}
          />
        </HStack>
        {error && <Banner status="error" title={error} />}
        {rows.length === 0 ? (
          <EmptyState
            icon={<FolderIcon width={32} />}
            title={t("categories.empty_title")}
            description={t("categories.empty_description")}
          />
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHeaderCell>{t("common.category")}</TableHeaderCell>
                <TableHeaderCell>{t("common.handle")}</TableHeaderCell>
                <TableHeaderCell>
                  <span />
                </TableHeaderCell>
              </TableRow>
            </TableHeader>
            <TableBody>
              {rows.map((row) => (
                <TableRow key={row.id}>
                  <TableCell>
                    <HStack gap={2} vAlign="center">
                      {/* Indentation shows the depth. */}
                      <span style={{ display: "inline-block", width: row.depth * 24 }} />
                      <Link href={`/products/categories/${row.id}`} weight="medium">
                        {row.name}
                      </Link>
                      {!row.visible ? (
                        <Badge label={t("categories.hidden")} />
                      ) : (
                        row.isHiddenInStore && <Badge label={t("categories.hidden_by_ancestor")} />
                      )}
                    </HStack>
                  </TableCell>
                  <TableCell>
                    <Text color="secondary">/{row.handle}</Text>
                  </TableCell>
                  <TableCell>
                    <HStack gap={1} hAlign="end">
                      <IconButton
                        label={t("categories.move_up", { name: row.name })}
                        icon={<ArrowUpIcon />}
                        size="sm"
                        variant="ghost"
                        isDisabled={isPending || row.position === 0}
                        onClick={() => move(row.id, row.position - 1)}
                      />
                      <IconButton
                        label={t("categories.move_down", { name: row.name })}
                        icon={<ArrowDownIcon />}
                        size="sm"
                        variant="ghost"
                        isDisabled={isPending || row.position >= row.siblings - 1}
                        onClick={() => move(row.id, row.position + 1)}
                      />
                    </HStack>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}
      </VStack>
      {isAdding && <AddCategoryDialog tree={tree} onClose={() => setIsAdding(false)} />}
    </Card>
  );
}

/** A new category: name, handle, parent, and whether it shows (Hidden preselected). */
function AddCategoryDialog({ tree, onClose }: { tree: CategoryNode[]; onClose: () => void }) {
  const t = useT<"categories">();
  const [name, setName] = useState("");
  // Follows the name until edited by hand; clearing it follows the name again.
  const [handle, setHandle] = useState<string>();
  const [parent, setParent] = useState(TOP_LEVEL);
  const [visibility, setVisibility] = useState<string>(VISIBILITY[0].value);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const effectiveHandle = handle ?? toHandle(name);
  const isComplete = name.trim() !== "" && effectiveHandle !== "";

  const submit = () => {
    if (!isComplete) return;
    startTransition(async () => {
      const result = await createCategory({
        name,
        handle: effectiveHandle,
        parent: parent === TOP_LEVEL ? null : parent,
        visible: visibility === "visible",
      });
      if (result.error) setError(result.error);
    });
  };

  return (
    <Dialog isOpen onOpenChange={(isOpen) => !isOpen && onClose()} width={520} purpose="form">
      <DialogHeader
        title={t("categories.add")}
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <TextInput
          label={t("common.name")}
          placeholder={t("categories.name_placeholder")}
          value={name}
          onChange={setName}
          hasAutoFocus
        />
        <TextInput
          label={t("common.handle")}
          description={t("categories.add_handle_description")}
          value={effectiveHandle}
          onChange={(value) => setHandle(value === "" ? undefined : value)}
        />
        <ParentSelector tree={tree} value={parent} onChange={setParent} />
        <Selector
          label={t("categories.visibility")}
          options={visibilityOptions(t)}
          value={visibility}
          onChange={setVisibility}
        />
        <HStack gap={2} hAlign="end">
          <Button label={t("common.cancel")} variant="ghost" onClick={onClose} />
          <Button
            label={t("categories.add")}
            variant="primary"
            isDisabled={!isComplete}
            isLoading={isPending}
            onClick={submit}
          />
        </HStack>
      </VStack>
    </Dialog>
  );
}

/** "Top level" or a category, by path; `exclude` can't be chosen (a category and its
 * descendants, when moving it). */
export function ParentSelector({
  tree,
  value,
  onChange,
  exclude,
}: {
  tree: CategoryNode[];
  value: string;
  onChange: (parent: string) => void;
  exclude?: Set<string>;
}) {
  const t = useT<"categories">();
  const options = [
    { value: TOP_LEVEL, label: t("categories.top_level") },
    ...flatten(tree)
      .filter((row) => !exclude?.has(row.id))
      .map((row) => ({ value: row.id, label: row.path })),
  ];
  return (
    <Selector
      label={t("categories.parent")}
      options={options}
      value={value}
      onChange={onChange}
      hasSearch
    />
  );
}
