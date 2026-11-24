// Shipping option rates as the admin edits them, and the API's tagged `rate` both
// ways. One built-in rate in the API (a basis and tiers per currency) is shown as the
// preset it matches: flat, free from an amount, tiers by subtotal or by weight. Shared
// by server and client components: what builds text takes the caller's translator.

import type { AdminSchemas } from "@hyphacommerce/sdk/admin";

import { type Currencies, type Format, moneyInput, parseMoney } from "./format";
import type { TFor } from "./messages";

type Currency = AdminSchemas["Currency"];
export type RateDto = AdminSchemas["RateDto"];

/** A tier as typed: the upper bound (money for subtotal, grams for weight; blank on the
 * last) and its price. */
export type TierDraft = { upTo: string; price: string };

export type RateDraft =
  | { kind: "flat"; prices: { currency: Currency; price: string }[] }
  | { kind: "free_over"; tables: { currency: Currency; price: string; freeFrom: string }[] }
  | { kind: "subtotal" | "weight"; tables: { currency: Currency; tiers: TierDraft[] }[] }
  | { kind: "provider"; provider: string; service: string };

export type RateKind = RateDraft["kind"];

/** Every kind, as the picker lists them: `shipping.rate_kind_<kind>` names each, and
 * `…_description` says what it is. */
export const RATE_KINDS: RateKind[] = ["flat", "free_over", "subtotal", "weight", "provider"];

/** A new draft of `kind`, in `currency`. */
export function emptyDraft(kind: RateKind, currency: Currency): RateDraft {
  switch (kind) {
    case "flat":
      return { kind, prices: [{ currency, price: "" }] };
    case "free_over":
      return { kind, tables: [{ currency, price: "", freeFrom: "" }] };
    case "subtotal":
    case "weight":
      return {
        kind,
        tables: [
          {
            currency,
            tiers: [
              { upTo: "", price: "" },
              { upTo: "", price: "" },
            ],
          },
        ],
      };
    case "provider":
      return { kind, provider: "", service: "" };
  }
}

/** The draft an option's rate shows as. */
export function toDraft(rate: RateDto, currencies: Currencies): RateDraft {
  if (rate.type === "provider") {
    return { kind: "provider", provider: rate.provider, service: rate.service };
  }
  const price = (currency: Currency, amount: number) =>
    moneyInput({ amount, currency }, currencies);
  if (rate.basis === "flat") {
    return {
      kind: "flat",
      prices: rate.tables.map((t) => ({
        currency: t.currency,
        price: price(t.currency, t.tiers[0].amount),
      })),
    };
  }
  const isFreeOver =
    rate.basis === "subtotal" &&
    rate.tables.every((t) => t.tiers.length === 2 && t.tiers[1].amount === 0);
  if (isFreeOver) {
    return {
      kind: "free_over",
      tables: rate.tables.map((t) => ({
        currency: t.currency,
        price: price(t.currency, t.tiers[0].amount),
        freeFrom: price(t.currency, t.tiers[0].up_to ?? 0),
      })),
    };
  }
  const basis = rate.basis;
  // The admin ships with the API it calls, so a basis it doesn't know is a bug here.
  if (basis !== "subtotal" && basis !== "weight") {
    throw new Error(`no editor for the rate basis ${JSON.stringify(basis)}`);
  }
  return {
    kind: basis,
    tables: rate.tables.map((t) => ({
      currency: t.currency,
      tiers: t.tiers.map((tier) => ({
        upTo:
          tier.up_to == null
            ? ""
            : basis === "weight"
              ? String(tier.up_to)
              : price(t.currency, tier.up_to),
        price: price(t.currency, tier.amount),
      })),
    })),
  };
}

/** The API's rate from a draft, or a message saying what to fix. The API checks the
 * rest (rising tiers, one table per currency) and answers `invalid_shipping_rate`. */
export function fromDraft(
  t: TFor<"shipping">,
  draft: RateDraft,
  currencies: Currencies,
): { rate: RateDto } | { error: string } {
  const money = (input: string, currency: Currency) => parseMoney(input, currency, currencies);
  const priceError = t("shipping.rate_error_price");
  switch (draft.kind) {
    case "provider":
      if (!draft.provider || !draft.service.trim()) {
        return { error: t("shipping.rate_error_provider") };
      }
      return {
        rate: { type: "provider", provider: draft.provider, service: draft.service.trim() },
      };
    case "flat": {
      const tables = [];
      for (const { currency, price } of draft.prices) {
        const amount = money(price, currency);
        if (amount === null) return { error: priceError };
        tables.push({ currency, tiers: [{ amount }] });
      }
      return { rate: { type: "builtin", basis: "flat", tables } };
    }
    case "free_over": {
      const tables = [];
      for (const { currency, price, freeFrom } of draft.tables) {
        const amount = money(price, currency);
        const from = money(freeFrom, currency);
        if (amount === null || from === null || from === 0) {
          return { error: t("shipping.rate_error_free_over") };
        }
        tables.push({ currency, tiers: [{ up_to: from, amount }, { amount: 0 }] });
      }
      return { rate: { type: "builtin", basis: "subtotal", tables } };
    }
    case "subtotal":
    case "weight": {
      const tables = [];
      for (const { currency, tiers } of draft.tables) {
        const parsed = [];
        for (const [i, tier] of tiers.entries()) {
          const amount = money(tier.price, currency);
          if (amount === null) return { error: priceError };
          const isLast = i === tiers.length - 1;
          if (isLast) {
            parsed.push({ amount });
            continue;
          }
          const upTo =
            draft.kind === "weight"
              ? /^\d+$/.test(tier.upTo.trim())
                ? Number(tier.upTo.trim())
                : null
              : money(tier.upTo, currency);
          if (upTo === null) {
            return {
              error:
                draft.kind === "weight"
                  ? t("shipping.rate_error_weight_limit")
                  : t("shipping.rate_error_subtotal_limit"),
            };
          }
          parsed.push({ up_to: upTo, amount });
        }
        tables.push({ currency, tiers: parsed });
      }
      return { rate: { type: "builtin", basis: draft.kind, tables } };
    }
  }
}

/** "€4.90", "€4.90, free from €50.00", "3 weight tiers (EUR)", "app_courier · express". */
export function describeRate(
  t: TFor<"shipping">,
  format: Format,
  rate: RateDto,
  currencies: Currencies,
): string {
  if (rate.type === "provider") return `${rate.provider} · ${rate.service}`;
  const draft = toDraft(rate, currencies);
  switch (draft.kind) {
    case "flat":
      return rate.tables
        .map((table) => format.money({ amount: table.tiers[0].amount, currency: table.currency }))
        .join(" · ");
    case "free_over":
      return rate.tables
        .map((table) =>
          t("shipping.rate_summary_free_over", {
            price: format.money({ amount: table.tiers[0].amount, currency: table.currency }),
            from: format.money({ amount: table.tiers[0].up_to ?? 0, currency: table.currency }),
          }),
        )
        .join(" · ");
    default: {
      const count = rate.tables[0]?.tiers.length ?? 0;
      const codes = format.list(
        rate.tables.map((table) => table.currency),
        "unit",
      );
      return draft.kind === "weight"
        ? t("shipping.rate_summary_weight_tiers", { count, currencies: codes })
        : t("shipping.rate_summary_subtotal_tiers", { count, currencies: codes });
    }
  }
}
