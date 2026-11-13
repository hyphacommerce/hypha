// Settings → Languages: how far each language covers the store's text, as a bar of what's
// translated, outdated, inherited from the language without its region and missing, each
// part a way into the worklist. A server
// component: it renders once per read and holds no state.

import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { Text } from "@astryxdesign/core/Text";
import type { ReactNode } from "react";

import type { Format } from "@/lib/format";
import { getFormat } from "@/lib/get-format";
import { requireStaff, sessionClient } from "@/lib/hypha";
import { getT } from "@/lib/i18n";
import { languageLabel, withoutRegion } from "@/lib/languages";
import type { T } from "@/lib/messages";
import { hasScope } from "@/lib/navigation";
import {
  type LanguageProgress,
  progressParts,
  shownShare,
  worklistHref,
} from "@/lib/translation-worklist";

/** The card, for a role that reads translations; nothing for one that doesn't. */
export async function TranslationProgress() {
  const me = await requireStaff();
  if (!hasScope(me.scopes, "translations:read")) return null;
  const client = await sessionClient();
  const [{ data }, t, format] = await Promise.all([
    client.GET("/admin/translations/progress"),
    getT(),
    getFormat(),
  ]);
  // A failed read leaves the card out: the rest of the page still works.
  if (!data || data.languages.length === 0) return null;
  const first = data.languages[0];
  return (
    <Card>
      <VStack gap={4}>
        <HStack gap={3} vAlign="start" hAlign="between" wrap="wrap">
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              {t("languages.progress_title")}
            </Text>
            <Text type="supporting" color="secondary">
              {t("languages.progress_description")}
            </Text>
          </VStack>
          <Button
            label={t("languages.progress_worklist")}
            href={worklistHref({ language: first.language })}
          />
        </HStack>
        {data.pending > 0 && (
          <Text type="supporting" color="secondary">
            {t("languages.progress_uncounted", { count: data.pending })}
          </Text>
        )}
        <VStack gap={4}>
          {data.languages.map((progress) => (
            <LanguageRow key={progress.language} progress={progress} t={t} format={format} />
          ))}
        </VStack>
      </VStack>
    </Card>
  );
}

function LanguageRow({
  progress,
  t,
  format,
}: {
  progress: LanguageProgress;
  t: T;
  format: Format;
}) {
  const parts = progressParts(progress);
  const name = languageLabel(format, progress.language);
  const tag = progress.language;
  // What a regional language reads where it has no translation of its own: `de` for
  // `de-AT`. Named only when something is inherited from it.
  const base = withoutRegion(tag);
  const inheritedFrom = base !== null && parts.inherited > 0 ? languageLabel(format, base) : null;
  return (
    <VStack gap={2}>
      <HStack gap={3} vAlign="center" hAlign="between" wrap="wrap">
        <Link href={worklistHref({ language: tag })} weight="semibold">
          {name}
        </Link>
        <Text type="supporting" color="secondary">
          {parts.translated === null
            ? t("languages.progress_nothing")
            : inheritedFrom !== null
              ? t("languages.progress_share_inherited", {
                  share: format.percent(shownShare(parts)),
                  inherited: parts.inherited,
                  base: inheritedFrom,
                })
              : t("languages.progress_share", {
                  share: format.percent(shownShare(parts)),
                  translated: parts.current + parts.outdated,
                  total: parts.total,
                })}
        </Text>
      </HStack>
      <div
        // The legend under it says the same in words.
        className="tl-bar"
        aria-hidden
      >
        <span className="tl-bar-current" style={{ width: `${parts.shares.current * 100}%` }} />
        <span className="tl-bar-outdated" style={{ width: `${parts.shares.outdated * 100}%` }} />
        <span className="tl-bar-inherited" style={{ width: `${parts.shares.inherited * 100}%` }} />
      </div>
      <HStack gap={4} wrap="wrap">
        <Legend part="current">{t("languages.progress_current", { count: parts.current })}</Legend>
        <Legend part="outdated">
          {parts.outdated > 0 ? (
            <Link href={worklistHref({ language: tag, state: "outdated" })}>
              {t("languages.progress_outdated", { count: parts.outdated })}
            </Link>
          ) : (
            t("languages.progress_outdated", { count: parts.outdated })
          )}
        </Legend>
        {inheritedFrom !== null && (
          <Legend part="inherited">
            {t("languages.progress_inherited", { count: parts.inherited, base: inheritedFrom })}
          </Legend>
        )}
        <Legend part="missing">
          {/* The bar's rest: what shoppers read in the default language. The worklist's
              "missing" is every field without a translation in exactly this language,
              the inherited ones too, so the link opens it as it is. */}
          {parts.missing > 0 ? (
            <Link href={worklistHref({ language: tag, state: "missing" })}>
              {t("languages.progress_missing", { count: parts.untranslated })}
            </Link>
          ) : (
            t("languages.progress_missing", { count: parts.untranslated })
          )}
        </Legend>
      </HStack>
    </VStack>
  );
}

/** One part of the legend, with the swatch of its colour on the bar. */
function Legend({
  part,
  children,
}: {
  part: "current" | "outdated" | "inherited" | "missing";
  children: ReactNode;
}) {
  return (
    <HStack gap={1} vAlign="center">
      <span className={`tl-swatch tl-swatch-${part}`} aria-hidden />
      <Text type="supporting">{children}</Text>
    </HStack>
  );
}
