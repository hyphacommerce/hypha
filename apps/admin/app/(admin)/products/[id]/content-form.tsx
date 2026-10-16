"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Text } from "@astryxdesign/core/Text";
import { TextArea } from "@astryxdesign/core/TextArea";
import { TextInput } from "@astryxdesign/core/TextInput";
import { Tokenizer } from "@astryxdesign/core/Tokenizer";
import type { SearchableItem, SearchSource } from "@astryxdesign/core/Typeahead";
import { useState, useTransition } from "react";

import { updateProduct } from "../actions";

/** Tags are typed in, not picked from a list: the source offers nothing, `hasCreate` adds. */
const noSuggestions: SearchSource = { search: () => [], bootstrap: () => [] };

const asItems = (tags: string[]): SearchableItem[] => tags.map((t) => ({ id: t, label: t }));

const firstWithLabel = (items: SearchableItem[], label: string) =>
  items.findIndex((item) => item.label.toLowerCase() === label.toLowerCase());

/**
 * What the product says about itself (design note 0019, section 2): description,
 * vendor, product type and tags, saved together (one PATCH). The description is HTML;
 * the API sanitises it, and the form then shows what was kept.
 */
export function ContentForm({
  id,
  description: savedDescription,
  vendor: savedVendor,
  productType: savedProductType,
  tags: savedTags,
}: {
  id: string;
  description: string;
  vendor: string;
  productType: string;
  tags: string[];
}) {
  const [description, setDescription] = useState(savedDescription);
  const [vendor, setVendor] = useState(savedVendor);
  const [productType, setProductType] = useState(savedProductType);
  const [tags, setTags] = useState(asItems(savedTags));
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();

  const tagLabels = tags.map((t) => t.label);
  const isDirty =
    description.trim() !== savedDescription ||
    vendor.trim() !== savedVendor ||
    productType.trim() !== savedProductType ||
    tagLabels.join("\n") !== savedTags.join("\n");

  const reset = () => {
    setDescription(savedDescription);
    setVendor(savedVendor);
    setProductType(savedProductType);
    setTags(asItems(savedTags));
    setError(undefined);
  };
  const save = () => {
    if (!isDirty) return;
    startTransition(async () => {
      const blankIsNull = (s: string) => (s.trim() === "" ? null : s);
      const result = await updateProduct(id, {
        description: blankIsNull(description),
        vendor: blankIsNull(vendor),
        product_type: blankIsNull(productType),
        tags: tagLabels,
      });
      setError(result.error);
    });
  };

  return (
    <Card>
      <VStack gap={4}>
        <Text type="display-3" as="h2">
          Content
        </Text>
        {error && <Banner status="error" title={error} />}
        <TextArea
          label="Description"
          description="HTML: paragraphs, headings, lists, links and images. Scripts and styles are removed when it's saved."
          isOptional
          rows={8}
          value={description}
          onChange={setDescription}
        />
        <HStack gap={4}>
          <TextInput label="Vendor" isOptional value={vendor} onChange={setVendor} onEnter={save} />
          <TextInput
            label="Product type"
            isOptional
            value={productType}
            onChange={setProductType}
            onEnter={save}
          />
        </HStack>
        <Tokenizer
          label="Tags"
          description="Shoppers can filter by them. Up to 250."
          isOptional
          placeholder="Add a tag…"
          searchSource={noSuggestions}
          hasCreate
          debounceMs={0}
          maxEntries={250}
          value={tags}
          onChange={(items) =>
            // The API keeps the first spelling of a tag; so does the form.
            setTags(items.filter((item, i) => firstWithLabel(items, item.label) === i))
          }
        />
        <HStack gap={2} hAlign="end">
          {isDirty && <Button label="Discard" variant="ghost" onClick={reset} />}
          <Button
            label="Save"
            variant="primary"
            isDisabled={!isDirty}
            isLoading={isPending}
            onClick={save}
          />
        </HStack>
      </VStack>
    </Card>
  );
}
