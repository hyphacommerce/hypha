"use client";

// A template's editor (design note 0028, §7): the subject, the HTML and the plain text,
// a live preview of the unsaved sources (against the sample or a chosen order), Save,
// Revert to default and Send test. Problems from a preview or a save point at their line.

import { AlertDialog } from "@astryxdesign/core/AlertDialog";
import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { SegmentedControl, SegmentedControlItem } from "@astryxdesign/core/SegmentedControl";
import { Spinner } from "@astryxdesign/core/Spinner";
import { Switch } from "@astryxdesign/core/Switch";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { useToast } from "@astryxdesign/core/Toast";
import { Tooltip } from "@astryxdesign/core/Tooltip";
import type { AdminSchemas } from "@hypha/sdk/admin";
import { useRef, useState, useTransition } from "react";

import { type Problem, describeProblem, variablePaths } from "@/lib/notifications";

import {
  type Sources,
  previewTemplate,
  revertTemplate,
  saveTemplate,
  sendTestEmail,
} from "../actions";
import { CodeEditor } from "./code-editor";
import { VariablesPanel } from "./variables-panel";

type Template = AdminSchemas["NotificationTemplateResponse"];
type Rendered = AdminSchemas["RenderedNotificationResponse"];

/** How long typing pauses before the preview renders again. */
const PREVIEW_DELAY_MS = 400;

const ORDER_ID = /^ord_[0-9a-f]{32}$/;

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

export function TemplateEditor({
  template,
  firstPreview,
  sender,
  canWrite,
  canReadOrders,
}: {
  template: Template;
  /** The saved sources rendered against the sample; `null` when they don't render. */
  firstPreview: Rendered | null;
  /** The deployment's sender: `log` refuses test sends. */
  sender: string;
  canWrite: boolean;
  /** Whether a preview can use a real order (`orders:read`). */
  canReadOrders: boolean;
}) {
  const key = template.key;
  const isLayout = key === "layout";
  const isStaffAccount =
    key === "staff_invite" || key === "staff_password_reset" || key === "customer_login_code";
  const [saved, setSaved] = useState(() => sourcesOf(template));
  const [draft, setDraft] = useState(saved);
  const [isCustomized, setIsCustomized] = useState(template.customized);
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

  const isDirty = !same(draft, saved);
  const orderId = ORDER_ID.test(order.trim()) ? order.trim() : null;
  const variables = variablePaths(template.variables);

  /** Renders `sources` once typing pauses; only the newest request's answer shows. */
  const schedulePreview = (sources: Sources, forOrder: string | null) => {
    clearTimeout(timer.current);
    setIsPreviewing(true);
    timer.current = setTimeout(async () => {
      const request = ++latest.current;
      try {
        const result = await previewTemplate(key, sources, forOrder);
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
      const result = await saveTemplate(key, draft);
      setProblem(result.problem);
      setError(result.error);
      if (!result.error) {
        setSaved(draft);
        setIsCustomized(true);
        toast({ body: "Template saved" });
      }
    });

  const discard = () => {
    setDraft(saved);
    schedulePreview(saved, orderId);
  };

  const sendTest = () =>
    startSending(async () => {
      const result = await sendTestEmail(key, draft, orderId);
      if (result.sentTo) {
        toast({ body: `Sent to ${result.sentTo}` });
      } else {
        setProblem(result.problem);
        setError(result.error);
      }
    });

  const canSend = sender !== "log";
  const textEditor = draft.text !== null;

  return (
    <VStack gap={4}>
      <div className="save-bar">
        <HStack gap={3} hAlign="between" vAlign="center" wrap="wrap">
          <HStack gap={2} vAlign="center">
            {isCustomized ? (
              <Badge label="Customized" variant="info" />
            ) : (
              <Badge label="Default" variant="neutral" />
            )}
            {!template.enabled && <Badge label="Off" variant="warning" />}
            <Text weight="medium">{isDirty ? "Unsaved changes" : "No unsaved changes"}</Text>
          </HStack>
          {canWrite && (
            <HStack gap={2} wrap="wrap">
              {isCustomized && (
                <Button
                  label="Revert to default"
                  variant="ghost"
                  onClick={() => setIsReverting(true)}
                />
              )}
              {isDirty && <Button label="Discard" variant="ghost" onClick={discard} />}
              <Tooltip
                content={
                  canSend
                    ? "Sends this preview to your own address."
                    : "Emails are only logged on this deployment: a test would reach nobody."
                }
              >
                <span>
                  <Button
                    label="Send test"
                    variant="secondary"
                    isDisabled={!canSend}
                    isLoading={isSending}
                    onClick={sendTest}
                  />
                </span>
              </Tooltip>
              <Button
                label="Save"
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
          title="These saved sources stopped working"
          description={`${describeProblem(template.problem)}. Emails use the default until you fix and save it, or revert.`}
        />
      )}
      {isLayout && (
        <Banner
          status="info"
          title="The frame of every email"
          description="Messages fill its {% block preheader %} and {% block content %}. Saving checks every email against it, so a layout that breaks one isn't saved."
        />
      )}

      <div className="template-columns">
        <VStack gap={4}>
          <Card>
            <VStack gap={4}>
              {!isLayout && (
                <TextInput
                  label="Subject"
                  description="One line of Jinja, like Order {{ order.number }} confirmed."
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
                <Text type="label">HTML</Text>
                {!isLayout && (
                  <Text type="supporting" color="secondary">
                    Starts with {'{% extends "layout" %}'} and fills the layout&apos;s blocks.
                  </Text>
                )}
                <CodeEditor
                  label="HTML source"
                  language="html"
                  value={draft.html}
                  onChange={(html) => edit({ html })}
                  errorAt={lineIn(problem, isLayout ? "layout" : "html") ?? lineIn(problem, "html")}
                  minHeight={360}
                />
              </VStack>
              {!isLayout && (
                <VStack gap={2}>
                  <Switch
                    label="Write the plain text yourself"
                    description="Off: the plain-text version is made from the HTML."
                    value={textEditor}
                    onChange={(isOn) => edit({ text: isOn ? (rendered?.text ?? "") : null })}
                  />
                  {textEditor && (
                    <CodeEditor
                      label="Plain-text source"
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
                  Preview
                </Text>
                {isPreviewing && <Spinner size="sm" aria-label="Rendering the preview" />}
              </HStack>
              <SegmentedControl label="Preview width" size="sm" value={width} onChange={setWidth}>
                <SegmentedControlItem value="desktop" label="Desktop" />
                <SegmentedControlItem value="mobile" label="Mobile" />
                <SegmentedControlItem value="text" label="Plain text" />
              </SegmentedControl>
            </HStack>
            {canReadOrders && !isStaffAccount && (
              <TextInput
                label="Preview with an order"
                description="An order ID (ord_…) from the orders page; blank: a made-up order."
                isOptional
                placeholder="ord_…"
                value={order}
                status={
                  order.trim() !== "" && !orderId
                    ? { type: "warning", message: "Not an order ID yet" }
                    : undefined
                }
                onChange={chooseOrder}
              />
            )}
            {rendered ? (
              <VStack gap={2}>
                {!isLayout && <Text weight="medium">Subject: {rendered.subject}</Text>}
                {problem && (
                  <Text type="supporting" color="secondary">
                    Showing the last preview that worked.
                  </Text>
                )}
                {width === "text" ? (
                  <pre className="email-text">{rendered.text}</pre>
                ) : (
                  <div className="email-frame">
                    {/* No scripts, forms or same-origin access: just the email. */}
                    <iframe
                      title="Email preview"
                      sandbox=""
                      srcDoc={rendered.html}
                      style={{ width: width === "mobile" ? 375 : "100%" }}
                    />
                  </div>
                )}
              </VStack>
            ) : (
              <Text color="secondary">
                {problem ? "Fix the problem above to see a preview." : "No preview yet."}
              </Text>
            )}
          </VStack>
        </Card>
      </div>

      {isReverting && (
        <RevertDialog
          templateKey={key}
          onClose={() => setIsReverting(false)}
          onReverted={(sources) => {
            setSaved(sources);
            setDraft(sources);
            setIsCustomized(false);
            setProblem(undefined);
            setError(undefined);
            schedulePreview(sources, orderId);
            toast({ body: "Back to the default" });
          }}
        />
      )}
    </VStack>
  );
}

function RevertDialog({
  templateKey,
  onClose,
  onReverted,
}: {
  templateKey: string;
  onClose: () => void;
  onReverted: (sources: Sources) => void;
}) {
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  return (
    <AlertDialog
      isOpen
      onOpenChange={(isOpen) => !isOpen && onClose()}
      title="Revert to the default?"
      description={
        error ??
        "Your changes to this template are deleted, and emails use Hypha's default from now on. Whether it's on stays as it is."
      }
      actionLabel="Revert"
      isActionLoading={isPending}
      onAction={() =>
        startTransition(async () => {
          const result = await revertTemplate(templateKey);
          if (result.error || !result.sources) {
            setError(result.error);
            return;
          }
          onReverted(result.sources);
          onClose();
        })
      }
    />
  );
}
