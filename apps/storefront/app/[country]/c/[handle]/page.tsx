import type { Metadata } from "next";

import { Listing, type ListingParams } from "@/components/listing";
import { api, countryOf, loaded } from "@/lib/api";
import { link } from "@/lib/site";

type Props = {
  params: Promise<{ country: string; handle: string }>;
  searchParams: Promise<ListingParams>;
};

async function load(handle: string) {
  return loaded(
    await api().GET("/storefront/categories/by-handle/{handle}", {
      params: { path: { handle } },
    }),
  );
}

export async function generateMetadata({ params }: Props): Promise<Metadata> {
  const category = await load((await params).handle);
  return { title: category.name };
}

/**
 * A category (design note 0023, §3): its breadcrumb and subcategories, then its
 * products, those of its subcategories included, through search (0022, CA3). A hidden
 * category is a 404.
 */
export default async function CategoryPage({ params, searchParams }: Props) {
  const { country: folder, handle } = await params;
  const country = await countryOf(folder);
  const code = country.country;
  const category = await load(handle);
  const breadcrumb = [...category.ancestors].reverse();

  return (
    <>
      <nav className="breadcrumb" aria-label="Breadcrumb">
        <a href={link(code, "/")}>Home</a>
        {breadcrumb.map((a) => (
          <span key={a.id}>
            {" / "}
            <a href={link(code, `/c/${a.handle}`)}>{a.name}</a>
          </span>
        ))}
      </nav>
      <h1>{category.name}</h1>
      {category.description && (
        <div
          className="description"
          // Sanitised by the API (scripts, styles and event attributes removed).
          dangerouslySetInnerHTML={{ __html: category.description }}
        />
      )}
      {category.children.length > 0 && (
        <div className="chips" style={{ margin: "16px 0 24px" }}>
          {category.children.map((c) => (
            <a key={c.id} className="chip" href={link(code, `/c/${c.handle}`)}>
              {c.name}
            </a>
          ))}
        </div>
      )}
      <Listing
        country={country}
        params={await searchParams}
        base={link(code, `/c/${handle}`)}
        category={category.id}
        productHref={(h) => link(code, `/p/${h}`)}
      />
    </>
  );
}
