// The extension points' request and response bodies by name (design note 0029, §3.1).
// The contract is hypha-extensions' DTOs, published as `openapi/extensions.json` and
// generated into `generated/extensions.ts` (`pnpm --filter @hypha/app generate`); these
// names alias the generated types, so they can't drift from what Hypha sends.

import type { components } from "./generated/extensions.ts";
import type { PointRequest, PointResponse } from "./extensions.ts";

type Schemas = components["schemas"];

/** Integer minor units and an ISO 4217 code, as everywhere in Hypha's API. */
export type Money = Schemas["Money"];
/** A postal address as Hypha sends it; personal fields are absent without their scopes (AP10). */
export type PostalAddress = Schemas["PostalAddressResponse"];

export type ShippingRatesRequest = PointRequest<"shipping.rates">;
export type ShippingRatesResponse = PointResponse<"shipping.rates">;

export type PaymentsStartRequest = PointRequest<"payments.start">;
export type PaymentsStartResponse = PointResponse<"payments.start">;
/** What the shopper does next: a redirect, a storefront element to mount, or instructions. */
export type NextAction = Schemas["NextAction"];
/** `payments.capture`, `.void` and `.refund` take the same request and answer alike. */
export type PaymentActionRequest = PointRequest<"payments.capture">;
export type PaymentActionResponse = PointResponse<"payments.capture">;

export type SearchQueryRequest = PointRequest<"search.query">;
export type SearchQueryResponse = PointResponse<"search.query">;

export type AdminActionRequest = PointRequest<"admin.action">;
export type AdminActionResponse = PointResponse<"admin.action">;
