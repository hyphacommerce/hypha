"use client";

// Getting an app in. A public app installs from its own site, which owns the OAuth flow
// (its PKCE verifier and redirect): "Install app" opens the app's install route, `GET
// <app_url>/install?api=<this store's API>` (docs/apps/admin-integration.md), and Hypha
// brings the staff member back to the consent page. A private app, a store's own
// integration, is its manifest uploaded here.

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { List, ListItem } from "@astryxdesign/core/List";
import { Text } from "@astryxdesign/core/Text";
import { TextArea } from "@astryxdesign/core/TextArea";
import { TextInput } from "@astryxdesign/core/TextInput";
import { useRouter } from "next/navigation";
import { type ChangeEvent, useState, useTransition } from "react";

import { type ManifestProblem, createPrivateApp } from "./extension-actions";

/**
 * The install URL for what staff typed: an app's address (its `app_url`) or its
 * manifest's URL (`…/hypha-app.json`, whose folder is taken as the app's address).
 */
export function installUrl(typed: string, api: string): string | null {
  let url: URL;
  try {
    url = new URL(typed.trim());
  } catch {
    return null;
  }
  const local = ["localhost", "127.0.0.1", "[::1]"].includes(url.hostname);
  if (url.protocol !== "https:" && !(url.protocol === "http:" && local)) return null;
  let base = url.pathname;
  if (base.endsWith(".json")) base = base.slice(0, base.lastIndexOf("/") + 1);
  if (!base.endsWith("/")) base = `${base}/`;
  const install = new URL(`${base}install`, url.origin);
  install.searchParams.set("api", api);
  return install.toString();
}

export function InstallButtons({ api, canWrite }: { api: string; canWrite: boolean }) {
  const [open, setOpen] = useState<"install" | "private">();
  if (!canWrite) return null;
  return (
    <>
      <Button label="Create private app" variant="secondary" onClick={() => setOpen("private")} />
      <Button label="Install app" variant="primary" onClick={() => setOpen("install")} />
      {open === "install" && <InstallDialog api={api} onClose={() => setOpen(undefined)} />}
      {open === "private" && <PrivateAppDialog onClose={() => setOpen(undefined)} />}
    </>
  );
}

function InstallDialog({ api, onClose }: { api: string; onClose: () => void }) {
  const [typed, setTyped] = useState("");
  const target = typed.trim() === "" ? null : installUrl(typed, api);
  return (
    <Dialog isOpen onOpenChange={(isOpen) => !isOpen && onClose()} width={560} purpose="form">
      <DialogHeader
        title="Install an app"
        subtitle="The app's own site starts the installation; you approve what it may do here."
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      <VStack gap={4} padding={4}>
        <TextInput
          label="The app's address"
          description="Its site (https://app.example) or its manifest's URL (…/hypha-app.json)."
          placeholder="https://"
          value={typed}
          onChange={setTyped}
        />
        {typed.trim() !== "" && !target && (
          <Banner
            status="warning"
            title="That isn't an app's address"
            description="Use an https:// URL (http:// only for localhost)."
          />
        )}
        {target && (
          <Text type="supporting" color="secondary">
            You&apos;ll continue on {new URL(target).host}, which sends you back here to approve.
            This store&apos;s API is {api}.
          </Text>
        )}
        <HStack gap={2} hAlign="end">
          <Button label="Cancel" variant="secondary" onClick={onClose} />
          <Button
            label="Continue to the app"
            variant="primary"
            isDisabled={!target}
            onClick={() => target && window.location.assign(target)}
          />
        </HStack>
      </VStack>
    </Dialog>
  );
}

const EXAMPLE = `{
  "key": "app_my_rates",
  "name": "My rates",
  "version": "1.0.0",
  "app_url": "https://rates.example.com",
  "redirect_uris": [],
  "scopes": ["shipping:read", "customer_data:address"],
  "scope_reasons": { "customer_data:address": "Quotes by postal code" },
  "privacy_url": "/privacy"
}`;

function PrivateAppDialog({ onClose }: { onClose: () => void }) {
  const router = useRouter();
  const [text, setText] = useState("");
  const [error, setError] = useState<string>();
  const [problems, setProblems] = useState<ManifestProblem[]>([]);
  const [pending, startTransition] = useTransition();

  const readFile = async (event: ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    if (file) setText(await file.text());
  };

  const submit = () =>
    startTransition(async () => {
      setError(undefined);
      setProblems([]);
      const result = await createPrivateApp(text);
      if ("request" in result) {
        router.push(`/apps/consent?request=${encodeURIComponent(result.request)}`);
        return;
      }
      setError(result.error);
      setProblems(result.problems ?? []);
    });

  return (
    <Dialog isOpen onOpenChange={(isOpen) => !isOpen && onClose()} width={640} purpose="form">
      <DialogHeader
        title="Create a private app"
        subtitle="Your own integration: its manifest says what it may do, you approve it, and its credentials are shown once."
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && (
          <Banner status="error" title={error} collapsible={false}>
            {problems.length > 0 && (
              <List>
                {problems.map((p) => (
                  <ListItem
                    key={`${p.path}:${p.message}`}
                    label={p.path || "manifest"}
                    description={p.message}
                  />
                ))}
              </List>
            )}
          </Banner>
        )}
        <VStack gap={1}>
          <Text type="label">Manifest file</Text>
          <input
            type="file"
            accept="application/json,.json"
            aria-label="Manifest file"
            onChange={readFile}
          />
        </VStack>
        <TextArea
          label="Or paste it"
          description="hypha-app.json: like a public app's, except it needs no redirect_uris."
          placeholder={EXAMPLE}
          value={text}
          onChange={setText}
          rows={12}
        />
        <HStack gap={2} hAlign="end">
          <Button label="Cancel" variant="secondary" onClick={onClose} />
          <Button
            label="Review access"
            variant="primary"
            isDisabled={text.trim() === ""}
            isLoading={pending}
            onClick={submit}
          />
        </HStack>
      </VStack>
    </Dialog>
  );
}
