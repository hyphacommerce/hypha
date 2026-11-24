import { EmptyState } from "@astryxdesign/core/EmptyState";
import { Center } from "@astryxdesign/core/Center";

import { getT } from "@/lib/i18n";

export default async function NotFound() {
  const t = await getT();
  return (
    <Center axis="both" padding={6} style={{ minHeight: "100%" }}>
      <EmptyState
        title={t("shell.not_found_title")}
        description={t("shell.not_found_description")}
      />
    </Center>
  );
}
