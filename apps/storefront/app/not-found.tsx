import { Button } from "@astryxdesign/core/Button";
import { EmptyState } from "@astryxdesign/core/EmptyState";

export default function NotFound() {
  return (
    <main className="shop-page shop-main">
      <EmptyState
        headingLevel={1}
        title="Not found"
        description="There's nothing here. It may have moved, or isn't sold here."
        actions={<Button label="Back to the shop" href="/" variant="primary" />}
      />
    </main>
  );
}
