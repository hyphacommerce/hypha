"use client";

// The cart's discount code field, in the drawer and on the cart page: a code and
// Apply, the refusal under the field (an invalid code), and the codes the cart keeps
// as removable tokens, each with what its state means ("Add €12.00 more to use
// WELCOME10"). A known code that doesn't apply yet stays, and applies once the cart
// qualifies.

import { Button } from "@astryxdesign/core/Button";
import { InputGroup } from "@astryxdesign/core/InputGroup";
import { VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { Token } from "@astryxdesign/core/Token";
import { useState, useTransition } from "react";

import { type Discounts, codeMessage, typedCode } from "@/lib/discounts";

import { useCart } from "./cart-provider";

export function DiscountCodeField({ discounts }: { discounts: Discounts }) {
  const { digits, applyCode, removeCode, isPending } = useCart();
  const [value, setValue] = useState("");
  const [error, setError] = useState<string>();
  const [applying, start] = useTransition();

  if (!discounts.takesCodes) return null;
  const apply = () => {
    const code = typedCode(value);
    if (!code) return;
    start(async () => {
      const refused = await applyCode(code);
      setError(refused);
      if (!refused) setValue("");
    });
  };

  return (
    <VStack gap={2}>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          apply();
        }}
      >
        <InputGroup
          label="Discount code"
          isLabelHidden
          size="sm"
          status={error ? { type: "error", message: error } : undefined}
        >
          <TextInput
            label="Discount code"
            placeholder="Discount code"
            autoComplete="off"
            value={value}
            onChange={(next) => {
              setValue(next);
              if (error) setError(undefined);
            }}
          />
          <Button
            label="Apply"
            type="submit"
            isLoading={applying}
            isDisabled={isPending || typedCode(value) === null}
          />
        </InputGroup>
      </form>
      {discounts.codes.length > 0 && (
        <VStack as="ul" gap={2} aria-label="Discount codes" className="shop-plain-list">
          {discounts.codes.map((code) => {
            const { text, tone } = codeMessage(code, discounts.applications, digits);
            return (
              <li key={code.code}>
                <VStack gap={1}>
                  <div>
                    <Token
                      label={code.code}
                      size="sm"
                      color={tone === "success" ? "green" : "default"}
                      isDisabled={isPending}
                      description={text}
                      onRemove={() => removeCode(code.code)}
                    />
                  </div>
                  <Text type="supporting" color={tone === "success" ? "accent" : "secondary"}>
                    {text}
                  </Text>
                </VStack>
              </li>
            );
          })}
        </VStack>
      )}
    </VStack>
  );
}
