// The extension points' request and response bodies by name. The contract is
// hypha-extensions' DTOs, published as `openapi/extensions.json` and generated into
// `generated/extensions.ts` (`pnpm --filter @hyphacommerce/app generate`); these names alias
// the generated types, so they can't drift from what Hypha sends.

import type { components } from "./generated/extensions.ts";
import type { PointRequest, PointResponse } from "./extensions.ts";

type Schemas = components["schemas"];

/** Integer minor units and an ISO 4217 code, as everywhere in Hypha's API. */
export type Money = Schemas["Money"];
/** A postal address as Hypha sends it; personal fields are absent without their scopes. */
export type PostalAddress = Schemas["PostalAddressResponse"];

export type ShippingRatesRequest = PointRequest<"shipping.rates">;
export type ShippingRatesResponse = PointResponse<"shipping.rates">;

/** Version 2: a cart's payment, started before its order exists. */
export type PaymentsStartRequest = PointRequest<"payments.start">;
export type PaymentsStartResponse = PointResponse<"payments.start">;
/** Where a payment the app started is, asked when the shopper says they paid. */
export type PaymentsStatusRequest = PointRequest<"payments.status">;
export type PaymentsStatusResponse = PointResponse<"payments.status">;
/** What the shopper does next: a redirect, a storefront element to mount, or instructions. */
export type NextAction = Schemas["NextAction"];
/** `payments.capture`, `.void` and `.refund` take the same request and answer alike. */
export type PaymentActionRequest = PointRequest<"payments.capture">;
export type PaymentActionResponse = PointResponse<"payments.capture">;

export type SearchQueryRequest = PointRequest<"search.query">;
export type SearchQueryResponse = PointResponse<"search.query">;

export type AdminActionRequest = PointRequest<"admin.action">;
export type AdminActionResponse = PointResponse<"admin.action">;
