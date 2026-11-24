import { Button } from "@astryxdesign/core/Button";
import { LanguageIcon } from "@heroicons/react/24/outline";

import type { TFor } from "@/lib/messages";
import { type Kind, translateHref } from "@/lib/translations";

/**
 * "Translate": the way from a resource's page, or its row in a list, to the translation
 * editor. Whoever shows it decides whether to (`offersTranslate`: the role reads the
 * kind's translations and the store has a language to translate into). Rendered from
 * server and client components alike, so the translator comes in.
 */
export function TranslateButton({
  t,
  kind,
  id,
  isCompact = false,
}: {
  t: TFor<"components">;
  kind: Kind;
  id: string;
  /** In a table row, beside its icon buttons. */
  isCompact?: boolean;
}) {
  return (
    <Button
      label={t("components.translate_action")}
      icon={<LanguageIcon />}
      href={translateHref(kind, id)}
      variant={isCompact ? "ghost" : "secondary"}
      size={isCompact ? "sm" : "md"}
    />
  );
}
