"use client";

// An app's embedded page (docs/apps/bridge.md): the app's own URL in a sandboxed
// iframe, answered through `@hypha/app-bridge-host`. Session tokens come from the
// admin's server (the staff session never reaches the app), navigation stays within
// the app's page here, toasts are the admin's, the product picker is the admin's own,
// and the frame takes the page's height.

import { Banner } from "@astryxdesign/core/Banner";
import { useToast } from "@astryxdesign/core/Toast";
import { BridgeHostError, attachBridge } from "@hypha/app-bridge-host";
import { useRouter } from "next/navigation";
import { useEffect, useEffectEvent, useRef, useState, useSyncExternalStore } from "react";

import { ProductPicker } from "@/components/product-picker";

import { appSessionToken } from "../../../extension-actions";

/** The iframe's `src`: the page's URL at `path`, with the admin's origin for the app. */
export function frameSrc(pageUrl: string, path: string, adminOrigin: string): string {
  const url = new URL(pageUrl);
  if (path !== "/") url.pathname = `${url.pathname.replace(/\/+$/, "")}${path}`;
  // The app checks it against `GET /admin/app-extensions/origins` before trusting it.
  url.searchParams.set("origin", adminOrigin);
  return url.toString();
}

/** The iframe's `src` and the origin the bridge answers, or `null` for a bad URL. */
function frameTarget(
  pageUrl: string,
  path: string,
  adminOrigin: string,
): { src: string; origin: string } | null {
  try {
    const src = frameSrc(pageUrl, path, adminOrigin);
    return { src, origin: new URL(src).origin };
  } catch {
    return null;
  }
}

/**
 * The bridge's contract (docs/apps/bridge.md). Scripts and the same origin together
 * are safe here because the page is the app's own origin, never the admin's:
 * same-origin gives it its own storage and cookies (its login), not ours.
 */
const SANDBOX = "allow-scripts allow-forms allow-same-origin allow-popups";

/** The origin never changes while the page is open. */
const noSubscription = () => () => {};

type Picking = { resolve: (ids: string[] | null) => void; multiple: boolean };

export function AppFrame({
  appKey,
  pageId,
  pageUrl,
  path,
  title,
}: {
  appKey: string;
  pageId: string;
  pageUrl: string;
  path: string;
  title: string;
}) {
  const router = useRouter();
  const toast = useToast();
  const frame = useRef<HTMLIFrameElement>(null);
  // The admin's own origin is known only in the browser: the iframe renders once it is.
  const adminOrigin = useSyncExternalStore(
    noSubscription,
    () => window.location.origin,
    () => null,
  );
  const target = adminOrigin ? frameTarget(pageUrl, path, adminOrigin) : null;
  const src = target?.src;
  const appOrigin = target?.origin;
  const [height, setHeight] = useState(600);
  const [picking, setPicking] = useState<Picking>();

  const getToken = useEffectEvent(async () => {
    const { token, error } = await appSessionToken(appKey);
    if (!token) throw new BridgeHostError("failed", error ?? "no session token");
    return token;
  });
  const navigate = useEffectEvent((to: string) => {
    const query = to === "/" ? "" : `?path=${encodeURIComponent(to)}`;
    router.push(`/apps/${encodeURIComponent(appKey)}/pages/${encodeURIComponent(pageId)}${query}`);
  });
  const showToast = useEffectEvent((message: string, tone: string) => {
    toast({ body: message, type: tone === "error" ? "error" : "info" });
  });
  const close = useEffectEvent(() => router.push(`/apps/${encodeURIComponent(appKey)}`));

  useEffect(() => {
    const iframe = frame.current;
    if (!iframe || !src || !appOrigin) return;
    return attachBridge(iframe, {
      appOrigin,
      getSessionToken: () => getToken(),
      navigate: (to) => navigate(to),
      toast: (message, { tone }) => showToast(message, tone),
      pickResource: ({ type, multiple }) => {
        if (type !== "product") {
          throw new BridgeHostError("unsupported", `this admin picks products only, not ${type}s`);
        }
        return new Promise((resolve) => setPicking({ resolve, multiple }));
      },
      onResize: (next) => setHeight(Math.max(120, Math.ceil(next))),
      onClose: () => close(),
    });
  }, [src, appOrigin]);

  const finishPicking = (ids: string[] | null) => {
    picking?.resolve(ids);
    setPicking(undefined);
  };

  if (adminOrigin && !target) {
    return <Banner status="error" title="This app page's address isn't valid" />;
  }
  return (
    <>
      {src && (
        <iframe
          ref={frame}
          src={src}
          title={title}
          sandbox={SANDBOX}
          style={{
            width: "100%",
            height,
            border: "1px solid var(--color-border, #e5e5e5)",
            borderRadius: 8,
            background: "white",
          }}
        />
      )}
      {picking && (
        <ProductPicker
          title="Choose for the app"
          mode={picking.multiple ? "multiple" : "single"}
          confirmLabel={(n) => (n > 1 ? `Choose ${n} products` : "Choose product")}
          onConfirm={async (products) => {
            finishPicking(products.map((p) => p.id));
          }}
          onClose={() => finishPicking(null)}
        />
      )}
    </>
  );
}
