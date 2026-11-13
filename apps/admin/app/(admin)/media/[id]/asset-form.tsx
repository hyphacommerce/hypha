"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { TextInput } from "@astryxdesign/core/TextInput";
import { startTransition, useActionState, useState } from "react";

import { useT } from "@/lib/use-t";

import { updateAsset } from "../actions";

/** Alt text is for images (`hasAlt`); every asset has a title. */
export function AssetForm(props: { id: string; alt: string; title: string; hasAlt: boolean }) {
  const t = useT<"media">();
  const [alt, setAlt] = useState(props.alt);
  const [title, setTitle] = useState(props.title);
  const [state, save, isPending] = useActionState(updateAsset, {});
  const isChanged = alt.trim() !== props.alt || title.trim() !== props.title;

  const submit = () => {
    if (!isChanged) return;
    startTransition(() => save({ id: props.id, alt: props.hasAlt ? alt : undefined, title }));
  };

  return (
    <VStack gap={4}>
      {state.error && <Banner status="error" title={state.error} />}
      {state.saved && !isChanged && <Banner status="success" title={t("media.saved")} />}
      {props.hasAlt && (
        <TextInput
          label={t("media.alt_text")}
          description={t("media.alt_description")}
          value={alt}
          onChange={setAlt}
          onEnter={submit}
        />
      )}
      <TextInput
        label={t("common.title")}
        description={t("media.title_description")}
        value={title}
        onChange={setTitle}
        onEnter={submit}
      />
      <HStack hAlign="end">
        <Button
          label={t("common.save")}
          variant="primary"
          isLoading={isPending}
          isDisabled={!isChanged}
          onClick={submit}
        />
      </HStack>
    </VStack>
  );
}
