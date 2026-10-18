import { permanentRedirect } from "next/navigation";

/** An app's deliveries moved under Apps; old links (bookmarks, emails) still land there. */
export default async function MovedAppDeliveries({
  params,
  searchParams,
}: {
  params: Promise<{ key: string }>;
  searchParams: Promise<Record<string, string | string[] | undefined>>;
}) {
  const [{ key }, query] = await Promise.all([params, searchParams]);
  const kept = new URLSearchParams();
  for (const name of ["status", "cursor"]) {
    const value = query[name];
    if (typeof value === "string") kept.set(name, value);
  }
  const search = kept.size > 0 ? `?${kept.toString()}` : "";
  permanentRedirect(`/apps/${encodeURIComponent(key)}/deliveries${search}`);
}
