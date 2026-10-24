/**
 * Structured data for search engines, as a JSON-LD script. `<` is escaped so no value
 * (a product title, a description) can close the script element early.
 */
export function JsonLd({ data }: { data: object | null }) {
  if (!data) return null;
  return (
    <script
      type="application/ld+json"
      // JSON with `<` escaped: nothing in it can end the script.
      dangerouslySetInnerHTML={{ __html: JSON.stringify(data).replace(/</g, "\\u003c") }}
    />
  );
}
