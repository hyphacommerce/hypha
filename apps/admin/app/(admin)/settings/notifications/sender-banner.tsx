import { Banner } from "@astryxdesign/core/Banner";
import { Text } from "@astryxdesign/core/Text";

import { getT } from "@/lib/i18n";

/** The senders' own names: the same in every language. */
const SENDERS: Record<string, string> = { smtp: "SMTP", resend: "Resend" };

/**
 * Where emails go (the operator's `[email]` settings): a line, or a warning when they're
 * only written to the log.
 */
export async function SenderBanner({ sender, from }: { sender: string; from: string }) {
  const t = await getT();
  if (sender === "log") {
    return (
      <Banner
        status="warning"
        title={t("notifications.sender_log_title")}
        description={t("notifications.sender_log_description", { from })}
      />
    );
  }
  return (
    <Text type="supporting" color="secondary">
      {t("notifications.sender_line", { from, sender: SENDERS[sender] ?? sender })}
    </Text>
  );
}
