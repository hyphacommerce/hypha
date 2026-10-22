"use client";

// Answering a consent request. An approved or denied public app's request answers
// where to send the browser (the app's redirect URI, with a code or
// `error=access_denied`): the browser goes there, leaving the admin. A private app's
// approval answers with its credentials, shown here once.

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { CodeBlock } from "@astryxdesign/core/CodeBlock";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Text } from "@astryxdesign/core/Text";
import { useRouter } from "next/navigation";
import { type ReactNode, useState, useTransition } from "react";

import {
  type ConsentResult,
  type Credentials,
  approveConsent,
  denyConsent,
} from "../extension-actions";

export function ConsentButtons({
  id,
  appKey,
  appName,
  isPending,
  canApprove,
  canDeny,
  intro,
}: {
  id: string;
  appKey: string;
  appName: string;
  /** Whether the request can still be answered. */
  isPending: boolean;
  canApprove: boolean;
  canDeny: boolean;
  /** What approving does, or why this staff member can't. */
  intro: ReactNode;
}) {
  const router = useRouter();
  const [error, setError] = useState<string>();
  const [credentials, setCredentials] = useState<Credentials>();
  const [leaving, setLeaving] = useState(false);
  const [pending, startTransition] = useTransition();

  const answer = (run: (id: string) => Promise<ConsentResult>) =>
    startTransition(async () => {
      setError(undefined);
      const result = await run(id);
      if ("error" in result) setError(result.error);
      else if ("credentials" in result) setCredentials(result.credentials);
      else if ("redirectTo" in result) {
        setLeaving(true);
        // Another site (the app's): a full navigation, not the admin's router.
        window.location.assign(result.redirectTo);
      } else router.push(`/apps/${appKey}`);
    });

  if (credentials) {
    return (
      <Card>
        <CredentialsShown name={appName} credentials={credentials} />
      </Card>
    );
  }
  if (leaving) return <Text color="secondary">Taking you back to {appName}…</Text>;
  if (!isPending) return null;
  return (
    <Card>
      <VStack gap={3}>
        {intro}
        {error && <Banner status="error" title={error} />}
        <HStack gap={2}>
          <Button
            label="Approve"
            variant="primary"
            isDisabled={!canApprove}
            isLoading={pending}
            onClick={() => answer(approveConsent)}
          />
          <Button
            label="Deny"
            variant="secondary"
            isDisabled={!canDeny || pending}
            onClick={() => answer(denyConsent)}
          />
        </HStack>
      </VStack>
    </Card>
  );
}

/** A private app's tokens and secret, once: the integration's server keeps them. */
export function CredentialsShown({
  name,
  credentials,
}: {
  name: string;
  credentials: Credentials;
}) {
  const env = [
    `HYPHA_APP_KEY=${credentials.app}`,
    `HYPHA_ACCESS_TOKEN=${credentials.access_token}`,
    `HYPHA_REFRESH_TOKEN=${credentials.refresh_token}`,
    `HYPHA_TOKEN_ENDPOINT=${credentials.token_endpoint}`,
    ...(credentials.app_secret ? [`HYPHA_APP_SECRET=${credentials.app_secret}`] : []),
  ].join("\n");
  return (
    <VStack gap={3}>
      <Banner
        status="success"
        title={`${name} is installed`}
        description="Copy its credentials now: they aren't shown again."
      />
      <CodeBlock code={env} title="Credentials" isWrapped />
      <Text type="supporting" color="secondary">
        The access token works for an hour. Before it lapses, exchange the refresh token at{" "}
        {credentials.token_endpoint} (grant_type=refresh_token) for a new pair; each refresh token
        is used once and lasts 30 days.
        {credentials.app_secret
          ? " The app secret verifies Hypha's webhook and extension-point signatures."
          : " The app keeps the secret it had."}
      </Text>
      <Banner
        status="warning"
        title="Keep them secret"
        description="They act as the app with the scopes you approved: store them in your server's secrets, never in a browser."
      />
      <HStack>
        <Button label="Done" variant="primary" href={`/apps/${credentials.app}`} />
      </HStack>
    </VStack>
  );
}
