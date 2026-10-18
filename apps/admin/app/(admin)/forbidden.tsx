import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { HStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";

/**
 * What a page shows when its API reads need a scope the user's role lacks
 * (`forbidden()`, from `loaded` in lib/hypha.ts): a notice inside the frame instead of
 * the error page, so the rest of the admin stays a click away.
 */
export default function Forbidden() {
  return (
    <Section padding={6}>
      <Banner
        status="info"
        title="Your role can't open this page"
        description="It needs access your role doesn't include. Ask the store owner to add it to your role. Your own account is always under Your account."
        endContent={
          <HStack gap={2}>
            <Button label="Home" variant="secondary" href="/" />
            <Button label="Your account" variant="secondary" href="/account" />
          </HStack>
        }
      />
    </Section>
  );
}
