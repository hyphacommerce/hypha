"use client";

// An app's storefront element in place: the custom element, loaded from the app's
// module in the browser, with its props set as properties. A failure to load shows
// nothing: an app's block never breaks the page.

import { HyphaElement } from "@hyphacommerce/storefront-elements/react";

export function AppElements({
  elements,
  props,
}: {
  elements: { app: string; id: string; tag: string; module: string }[];
  props: Record<string, unknown>;
}) {
  if (elements.length === 0) return null;
  return (
    <div className="shop-app-elements">
      {elements.map((element) => (
        <HyphaElement
          key={`${element.app}/${element.id}`}
          tag={element.tag}
          module={element.module}
          props={props}
        />
      ))}
    </div>
  );
}
