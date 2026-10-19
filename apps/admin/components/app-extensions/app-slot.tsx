// Server-only: one admin target's app contributions in place (design note 0029, §5):
// the blocks' trees fetched now (an app that fails shows a note, never an error), and
// the apps' action buttons for the target.

import { VStack } from "@astryxdesign/core/Layout";

import {
  type AdminTarget,
  type ExtensionContext,
  type Extensions,
  appPagePath,
  forTarget,
  loadBlocks,
} from "@/lib/app-extensions";

import { AppActionButtons, AppBlockCard } from "./app-ui";

export async function AppSlot({
  extensions,
  target,
  context,
  withActions = true,
}: {
  extensions: Extensions;
  target: AdminTarget;
  context: ExtensionContext;
  /** Show the target's action buttons below the blocks. */
  withActions?: boolean;
}) {
  const { blocks, actions } = forTarget(extensions, target);
  if (blocks.length === 0 && (!withActions || actions.length === 0)) return null;
  const loaded = await loadBlocks(extensions, blocks, context);
  const appNames = Object.fromEntries(extensions.apps.map((a) => [a.key, a.name]));
  return (
    <VStack gap={3}>
      {loaded.map((block) => (
        <AppBlockCard
          key={`${block.app}/${block.id}`}
          appKey={block.app}
          appName={block.appName}
          tree={block.tree}
          actions={actions.filter((a) => a.app === block.app)}
          context={context}
          pagePath={appPagePath(extensions, block.app, "/")}
        />
      ))}
      {withActions && <AppActionButtons actions={actions} context={context} appNames={appNames} />}
    </VStack>
  );
}

/** Just the apps' action buttons for a target (a payment's, beside its own). */
export function AppActions({
  extensions,
  target,
  context,
}: {
  extensions: Extensions;
  target: AdminTarget;
  context: ExtensionContext;
}) {
  const { actions } = forTarget(extensions, target);
  const appNames = Object.fromEntries(extensions.apps.map((a) => [a.key, a.name]));
  return <AppActionButtons actions={actions} context={context} appNames={appNames} />;
}
