"use client";

// A template's editor in one of the store's languages: the subject, the HTML and the
// plain text, a live preview of the unsaved sources (against the sample or a chosen
// order), Save, Revert to default and Send test, all in that language. Problems from a
// preview or a save point at their line.

import { AlertDialog } from "@astryxdesign/core/AlertDialog";
import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { SegmentedControl, SegmentedControlItem } from "@astryxdesign/core/SegmentedControl";
import { Selector } from "@astryxdesign/core/Selector";
import { Spinner } from "@astryxdesign/core/Spinner";
import { Switch } from "@astryxdesign/core/Switch";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { useToast } from "@astryxdesign/core/Toast";
import { Tooltip } from "@astryxdesign/core/Tooltip";
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { useRouter } from "next/navigation";
import { useEffect, useRef, useState, useTransition } from "react";

import { type StoreLanguage, languageLabel } from "@/lib/languages";
import type { TFor } from "@/lib/messages";
import { type Problem, describeProblem, templateHref, variablePaths } from "@/lib/notifications";
import { useFormat } from "@/lib/use-format";
import { useT } from "@/lib/use-t";

import {
  type Sources,
  type SourcesFrom,
  previewTemplate,
  revertTemplate,
  saveTemplate,
  sendTestEmail,
  setTemplateEnabled,
} from "../actions";
import { CodeEditor } from "./code-editor";
import { VariablesPanel } from "./variables-panel";

type Template = AdminSchemas["NotificationTemplateResponse"];
type Rendered = AdminSchemas["RenderedNotificationResponse"];

/** How long typing pauses before the preview renders again. */
const PREVIEW_DELAY_MS = 400;

const ORDER_ID = /^ord_[0-9a-f]{32}$/;
/** How an order ID starts: a code, the same in every language. */
const ORDER_ID_EXAMPLE = "ord_…";

function sourcesOf(template: Template): Sources {
  return { subject: template.subject, html: template.html, text: template.text ?? null };
}

function same(a: Sources, b: Sources): boolean {
  return a.subject === b.subject && a.html === b.html && a.text === b.text;
}

/** The line to mark in the editor of `part`, if the problem is there. */
function lineIn(problem: Problem | undefined, part: string): number | null {
  return problem?.part === part ? (problem.line ?? null) : null;
}

/** The store's languages a template can be written in. */
type Languages = {
  /** The content language: the template's own, and where it's turned on or off. */
  content: string;
  /** The others, published or not. */
  others: StoreLanguage[];
};

type EditorT = TFor<"notifications">;

/** "Showing your English template": whose the shown sources are, and in which language. */
function sourcesLine(t: EditorT, from: SourcesFrom, name: (tag: string) => string) {
  const language = name(from.language);
  if (from.origin === "merchant") return t("notifications.sources_merchant", { language });
  if (from.origin === "default") return t("notifications.sources_default", { language });
  return undefined;
}

export function TemplateEditor({
  template,
  language,
  languages,
  firstPreview,
  sender,
  canWrite,
  canReadOrders,
}: {
  /** The template in `language`, as the API answers it. */
  template: Template;
  /** The language edited: one of `languages.others`, or `null` for the content language. */
  language: string | null;
  /** The languages to switch between; `null` hides the switch (one language, or a staff
   * or supplier email, which has one template). */
  languages: Languages | null;
  /** The saved sources rendered against the sample; `null` when they don't render. */
  firstPreview: Rendered | null;
  /** The deployment's sender: `log` refuses test sends. */
  sender: string;
  canWrite: boolean;
  /** Whether a preview can use a real order (`orders:read`). */
  canReadOrders: boolean;
}) {
  const t = useT<"notifications">();
  const format = useFormat();
  const router = useRouter();
  const key = template.key;
  const isLayout = key === "layout";
  // Templates about no order: their preview is the sample's.
  const isStaffAccount =
    key === "staff_invite" ||
    key === "staff_password_reset" ||
    key === "customer_login_code" ||
    key === "customer_erasure_requested" ||
    key === "supplier_purchase_order";
  const [saved, setSaved] = useState(() => sourcesOf(template));
  const [draft, setDraft] = useState(saved);
  const [isCustomized, setIsCustomized] = useState(template.customized);
  const [from, setFrom] = useState<SourcesFrom>({
    language: template.sources_language,
    origin: template.sources_origin,
  });
  const [isEnabled, setIsEnabled] = useState(template.enabled);
  const [isSwitching, startSwitching] = useTransition();
  const [order, setOrder] = useState("");
  const [rendered, setRendered] = useState(firstPreview);
  const [problem, setProblem] = useState<Problem>();
  const [error, setError] = useState<string>();
  const [isPreviewing, setIsPreviewing] = useState(false);
  const [width, setWidth] = useState("desktop");
  const [isReverting, setIsReverting] = useState(false);
  const [isSaving, startSaving] = useTransition();
  const [isSending, startSending] = useTransition();
  const toast = useToast();
  // The pending preview's timer, and the newest request: an older answer is dropped.
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const latest = useRef(0);
  // Another language remounts the editor: a preview still waiting is the old one's.
  useEffect(() => () => clearTimeout(timer.current), []);

  const isDirty = !same(draft, saved);
  const orderId = ORDER_ID.test(order.trim()) ? order.trim() : null;
  const variables = variablePaths(t, template.variables);

  /** Renders `sources` once typing pauses; only the newest request's answer shows. */
  const schedulePreview = (sources: Sources, forOrder: string | null) => {
    clearTimeout(timer.current);
    setIsPreviewing(true);
    timer.current = setTimeout(async () => {
      const request = ++latest.current;
      try {
        const result = await previewTemplate(key, language, sources, forOrder);
        if (request !== latest.current) return;
        if (result.rendered) {
          setRendered(result.rendered);
          setProblem(undefined);
          setError(undefined);
        } else {
          setProblem(result.problem);
          setError(result.error);
        }
      } finally {
        if (request === latest.current) setIsPreviewing(false);
      }
    }, PREVIEW_DELAY_MS);
  };

  const edit = (change: Partial<Sources>) => {
    const next = { ...draft, ...change };
    setDraft(next);
    schedulePreview(next, orderId);
  };

  const chooseOrder = (value: string) => {
    setOrder(value);
    const id = ORDER_ID.test(value.trim()) ? value.trim() : null;
    if (id !== orderId) schedulePreview(draft, id);
  };

  const save = () =>
    startSaving(async () => {
      const result = await saveTemplate(key, language, draft);
      setProblem(result.problem);
      setError(result.error);
      if (!result.error) {
        setSaved(draft);
        setIsCustomized(true);
        if (result.from) setFrom(result.from);
        toast({ body: t("notifications.editor_saved_toast") });
      }
    });

  const discard = () => {
    setDraft(saved);
    schedulePreview(saved, orderId);
  };

  const sendTest = () =>
    startSending(async () => {
      const result = await sendTestEmail(key, language, draft, orderId);
      if (result.sentTo) {
        toast({ body: t("notifications.editor_sent_toast", { email: result.sentTo }) });
      } else {
        setProblem(result.problem);
        setError(result.error);
      }
    });

  const toggleEnabled = async (enabled: boolean) => {
    const result = await setTemplateEnabled(key, enabled);
    // A failure is shown; a success leaves another edit's problem where it is.
    if (result.error) setError(result.error);
    else setIsEnabled(enabled);
  };

  const canSend = sender !== "log";
  const textEditor = draft.text !== null;
  // Each message puts the name inside its sentence.
  const languageName = (tag: string) => format.languageNameInSentence(tag);
  const contentName = languages ? languageName(languages.content) : undefined;
  const shownFrom = sourcesLine(t, from, languageName);

  return (
    // While another language loads, nothing here acts on the one being left.
    <div style={isSwitching ? { opacity: 0.6, pointerEvents: "none" } : undefined}>
      <VStack gap={4}>
        <div className="save-bar">
          <HStack gap={3} hAlign="between" vAlign="center" wrap="wrap">
            <HStack gap={2} vAlign="center">
              {isCustomized ? (
                <Badge label={t("notifications.template_customized")} variant="info" />
              ) : (
                <Badge label={t("common.default")} variant="neutral" />
              )}
              {!isEnabled && <Badge label={t("common.off")} variant="warning" />}
              <Text weight="medium">
                {isDirty
                  ? t("common.unsaved_changes")
                  : t("notifications.editor_no_unsaved_changes")}
              </Text>
            </HStack>
            {canWrite && (
              <HStack gap={2} wrap="wrap">
                {isCustomized && (
                  <Button
                    label={t("notifications.editor_revert")}
                    variant="ghost"
                    onClick={() => setIsReverting(true)}
                  />
                )}
                {isDirty && (
                  <Button label={t("common.discard")} variant="ghost" onClick={discard} />
                )}
                <Tooltip
                  content={
                    canSend
                      ? t("notifications.editor_send_test_tooltip")
                      : t("notifications.editor_send_test_logged")
                  }
                >
                  <span>
                    <Button
                      label={t("notifications.editor_send_test")}
                      variant="secondary"
                      isDisabled={!canSend}
                      isLoading={isSending}
                      onClick={sendTest}
                    />
                  </span>
                </Tooltip>
                <Button
                  label={t("common.save")}
                  variant="primary"
                  isDisabled={!isDirty}
                  isLoading={isSaving}
                  onClick={save}
                />
              </HStack>
            )}
          </HStack>
          {error && (
            <div style={{ marginTop: 12 }}>
              <Banner
                status="error"
                title={error}
                onDismiss={() => {
                  setError(undefined);
                  setProblem(undefined);
                }}
              />
            </div>
          )}
        </div>

        {template.problem && !isDirty && (
          <Banner
            status="error"
            title={t("notifications.editor_broken_title")}
            description={t("notifications.editor_broken_description", {
              problem: describeProblem(t, template.problem),
            })}
          />
        )}
        {isLayout && (
          <Banner
            status="info"
            title={t("notifications.editor_layout_title")}
            description={t("notifications.editor_layout_description")}
          />
        )}

        <Card>
          <VStack gap={4}>
            <HStack gap={4} vAlign="end" wrap="wrap">
              {languages && (
                <Selector
                  label={t("notifications.language_label")}
                  options={[languages.content, ...languages.others.map((l) => l.language)].map(
                    (tag) => ({
                      value: tag,
                      label:
                        tag === languages.content
                          ? t("notifications.language_option_content", {
                              language: languageLabel(format, tag),
                            })
                          : languages.others.some((l) => l.language === tag && !l.published)
                            ? t("notifications.language_option_unpublished", {
                                language: languageLabel(format, tag),
                              })
                            : languageLabel(format, tag),
                    }),
                  )}
                  value={language ?? languages.content}
                  onChange={(tag) =>
                    // The same page in another language: replaced in place, never a push.
                    startSwitching(() =>
                      router.replace(templateHref(key, tag === languages.content ? null : tag), {
                        scroll: false,
                      }),
                    )
                  }
                  hasSearch={languages.others.length > 8}
                  isLoading={isSwitching}
                  isDisabled={isDirty}
                  disabledMessage={t("notifications.language_locked")}
                  width={320}
                />
              )}
              <VStack gap={0.5}>
                {shownFrom && <Text type="supporting">{shownFrom}</Text>}
                {language !== null && !isCustomized && (
                  <Text type="supporting" color="secondary">
                    {t("notifications.sources_fallback", { language: languageName(language) })}
                  </Text>
                )}
              </VStack>
            </HStack>
            {template.can_disable &&
              (language === null ? (
                <Switch
                  label={t("notifications.sending_label")}
                  description={languages ? t("notifications.sending_description") : undefined}
                  value={isEnabled}
                  isDisabled={!canWrite}
                  changeAction={toggleEnabled}
                />
              ) : (
                <Text type="supporting" color="secondary">
                  {t("notifications.sending_other_language", { language: contentName ?? "" })}
                </Text>
              ))}
          </VStack>
        </Card>

        <div className="template-columns">
          <VStack gap={4}>
            <Card>
              <VStack gap={4}>
                {!isLayout && (
                  <TextInput
                    label={t("notifications.editor_subject_label")}
                    description={t("notifications.editor_subject_description")}
                    value={draft.subject}
                    status={
                      problem?.part === "subject"
                        ? { type: "error", message: problem.message }
                        : undefined
                    }
                    onChange={(subject) => edit({ subject })}
                  />
                )}
                <VStack gap={1}>
                  <Text type="label">{t("notifications.editor_html_label")}</Text>
                  {!isLayout && (
                    <Text type="supporting" color="secondary">
                      {t("notifications.editor_html_hint")}
                    </Text>
                  )}
                  <CodeEditor
                    label={t("notifications.editor_html_source")}
                    language="html"
                    value={draft.html}
                    onChange={(html) => edit({ html })}
                    errorAt={
                      lineIn(problem, isLayout ? "layout" : "html") ?? lineIn(problem, "html")
                    }
                    minHeight={360}
                  />
                </VStack>
                {!isLayout && (
                  <VStack gap={2}>
                    <Switch
                      label={t("notifications.editor_text_switch")}
                      description={t("notifications.editor_text_switch_description")}
                      value={textEditor}
                      onChange={(isOn) => edit({ text: isOn ? (rendered?.text ?? "") : null })}
                    />
                    {textEditor && (
                      <CodeEditor
                        label={t("notifications.editor_text_source")}
                        language="text"
                        value={draft.text ?? ""}
                        onChange={(text) => edit({ text })}
                        errorAt={lineIn(problem, "text")}
                        minHeight={200}
                      />
                    )}
                  </VStack>
                )}
              </VStack>
            </Card>
            <VariablesPanel variables={variables} />
          </VStack>

          <Card>
            <VStack gap={3}>
              <HStack gap={2} hAlign="between" vAlign="center" wrap="wrap">
                <HStack gap={2} vAlign="center">
                  <Text type="display-3" as="h2">
                    {t("notifications.preview_title")}
                  </Text>
                  {isPreviewing && (
                    <Spinner size="sm" aria-label={t("notifications.preview_rendering")} />
                  )}
                </HStack>
                <SegmentedControl
                  label={t("notifications.preview_width")}
                  size="sm"
                  value={width}
                  onChange={setWidth}
                >
                  <SegmentedControlItem
                    value="desktop"
                    label={t("notifications.preview_desktop")}
                  />
                  <SegmentedControlItem value="mobile" label={t("notifications.preview_mobile")} />
                  <SegmentedControlItem value="text" label={t("notifications.preview_text")} />
                </SegmentedControl>
              </HStack>
              {canReadOrders && !isStaffAccount && (
                <TextInput
                  label={t("notifications.preview_order_label")}
                  description={t("notifications.preview_order_description")}
                  isOptional
                  placeholder={ORDER_ID_EXAMPLE}
                  value={order}
                  status={
                    order.trim() !== "" && !orderId
                      ? { type: "warning", message: t("notifications.preview_order_invalid") }
                      : undefined
                  }
                  onChange={chooseOrder}
                />
              )}
              {rendered ? (
                <VStack gap={2}>
                  {!isLayout && (
                    <Text weight="medium">
                      {t("notifications.preview_subject", { subject: rendered.subject })}
                    </Text>
                  )}
                  {problem && (
                    <Text type="supporting" color="secondary">
                      {t("notifications.preview_last_working")}
                    </Text>
                  )}
                  {width === "text" ? (
                    <pre className="email-text">{rendered.text}</pre>
                  ) : (
                    <div className="email-frame">
                      {/* No scripts, forms or same-origin access: just the email. */}
                      <iframe
                        title={t("notifications.preview_frame_title")}
                        sandbox=""
                        srcDoc={rendered.html}
                        style={{ width: width === "mobile" ? 375 : "100%" }}
                      />
                    </div>
                  )}
                </VStack>
              ) : (
                <Text color="secondary">
                  {problem
                    ? t("notifications.preview_fix_problem")
                    : t("notifications.preview_none")}
                </Text>
              )}
            </VStack>
          </Card>
        </div>

        {isReverting && (
          <RevertDialog
            templateKey={key}
            language={language}
            description={
              language !== null
                ? t("notifications.revert_dialog_description_language", {
                    language: languageName(language),
                  })
                : languages
                  ? t("notifications.revert_dialog_description_content")
                  : t("notifications.revert_dialog_description")
            }
            onClose={() => setIsReverting(false)}
            onReverted={(sources, reverted) => {
              setSaved(sources);
              setDraft(sources);
              setIsCustomized(false);
              if (reverted) setFrom(reverted);
              setProblem(undefined);
              setError(undefined);
              schedulePreview(sources, orderId);
              toast({ body: t("notifications.editor_reverted_toast") });
            }}
          />
        )}
      </VStack>
    </div>
  );
}

function RevertDialog({
  templateKey,
  language,
  description,
  onClose,
  onReverted,
}: {
  templateKey: string;
  /** The language whose template goes (`null`: the content language's). */
  language: string | null;
  description: string;
  onClose: () => void;
  onReverted: (sources: Sources, from: SourcesFrom | undefined) => void;
}) {
  const t = useT<"notifications">();
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  return (
    <AlertDialog
      isOpen
      onOpenChange={(isOpen) => !isOpen && onClose()}
      title={t("notifications.revert_dialog_title")}
      description={error ?? description}
      actionLabel={t("notifications.revert_dialog_action")}
      isActionLoading={isPending}
      onAction={() =>
        startTransition(async () => {
          const result = await revertTemplate(templateKey, language);
          if (result.error || !result.sources) {
            setError(result.error);
            return;
          }
          onReverted(result.sources, result.from);
          onClose();
        })
      }
    />
  );
}
