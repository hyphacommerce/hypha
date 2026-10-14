import { EmptyState } from "@astryxdesign/core/EmptyState";
import { Center } from "@astryxdesign/core/Center";

export default function NotFound() {
  return (
    <Center axis="both" padding={6} style={{ minHeight: "100%" }}>
      <EmptyState title="Page not found" description="There's nothing at this address." />
    </Center>
  );
}
