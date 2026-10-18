import { Banner } from "@astryxdesign/core/Banner";
import { Text } from "@astryxdesign/core/Text";

const SENDERS: Record<string, string> = { log: "the server log", smtp: "SMTP", resend: "Resend" };

/**
 * Where emails go (the operator's `[email]` settings, design note 0028 §5): a line, or a
 * warning when they're only written to the log.
 */
export function SenderBanner({ sender, from }: { sender: string; from: string }) {
  if (sender === "log") {
    return (
      <Banner
        status="warning"
        title="Emails are only written to the server log"
        description={`Nothing reaches customers or staff yet. Whoever runs this deployment sets a sender (SMTP or Resend) and the from address under [email] in hypha.config.toml. Emails will go out from ${from}.`}
      />
    );
  }
  return (
    <Text type="supporting" color="secondary">
      Emails go out from {from} via {SENDERS[sender] ?? sender}.
    </Text>
  );
}
