import { permanentRedirect } from "next/navigation";

/** Custom fields moved from Settings to Content; old links (bookmarks) still land there. */
export default async function MovedCustomFields({
  searchParams,
}: {
  searchParams: Promise<Record<string, string | string[] | undefined>>;
}) {
  const { owner } = await searchParams;
  const search = typeof owner === "string" ? `?owner=${encodeURIComponent(owner)}` : "";
  permanentRedirect(`/custom-fields${search}`);
}
