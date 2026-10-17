import { Button } from "@astryxdesign/core/Button";
import { EmptyState } from "@astryxdesign/core/EmptyState";
import { Section } from "@astryxdesign/core/Section";

export default function NotFound() {
  return (
    <Section padding={10}>
      <EmptyState
        headingLevel={1}
        title="Not found"
        description="There's nothing here. It may have moved, or isn't sold here."
        actions={<Button label="Back to the shop" href="/" variant="primary" />}
      />
    </Section>
  );
}
