// A product's, variant's or category's custom fields, as label and value pairs. The
// values arrive formatted (`lib/custom-fields.ts`'s `presentFields`), so this renders
// on the server or in the browser alike.

import { Link } from "@astryxdesign/core/Link";
import { MetadataList, MetadataListItem } from "@astryxdesign/core/MetadataList";
import { Text } from "@astryxdesign/core/Text";
import { Fragment } from "react";

import type { FieldEntry } from "@/lib/custom-fields";

function Value({ entry }: { entry: FieldEntry }) {
  if (entry.html !== undefined) {
    return (
      <div
        className="shop-prose"
        // Sanitised by the API, and only the tags `isRichText` allows.
        // react-doctor-disable-next-line react-doctor/dangerous-html-sink -- checked above
        dangerouslySetInnerHTML={{ __html: entry.html }}
      />
    );
  }
  if (entry.lines !== undefined) {
    return (
      <Text as="p">
        <span className="shop-lines">{entry.lines}</span>
      </Text>
    );
  }
  const parts = entry.parts ?? [];
  return (
    <Text>
      {parts.map((part, i) => (
        <Fragment key={part.key}>
          {i > 0 && ", "}
          {part.href ? <Link href={part.href}>{part.text}</Link> : part.text}
        </Fragment>
      ))}
    </Text>
  );
}

/** The fields as a list, or nothing when there are none. */
export function CustomFields({
  entries,
  title,
  columns,
}: {
  entries: FieldEntry[];
  title?: string;
  columns?: "single" | "multi";
}) {
  if (entries.length === 0) return null;
  return (
    <MetadataList title={title} columns={columns}>
      {entries.map((entry) => (
        <MetadataListItem key={entry.key} label={entry.label}>
          <Value entry={entry} />
        </MetadataListItem>
      ))}
    </MetadataList>
  );
}
