//! What every list route shares (design note 0012): the `cursor` and `limit` query
//! parameters, the cursor itself, and the page response. A new list takes a
//! `ListQuery<Filters>` (or `ListQuery` without filters), lists `PageParams` and its
//! filters in `params(...)`, and answers with a `PageResponse<ItemDto>`.
//!
//! Cursors are opaque to clients (design note 0016, section 9): base64url of a small
//! JSON object with a version, the last item's ID and the list's filters, so a cursor
//! only continues the list it came from, and a later sort order can add its sort key
//! without changing the API.

use std::{borrow::Cow, fmt::Display, marker::PhantomData, str::FromStr};

use axum::{extract::FromRequestParts, http::request::Parts};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hypha_core::{Page, PageLimit, PageRequest};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use utoipa::{
    PartialSchema, ToSchema,
    openapi::{
        ArrayBuilder, KnownFormat, ObjectBuilder, Ref, RefOr, Required, Schema, SchemaFormat, Type,
        path::{Parameter, ParameterBuilder, ParameterIn},
        schema::SchemaType,
    },
};

use crate::error::ApiError;

/// A page of a list, newest first. In the OpenAPI documents it is named after its
/// items (`ProductResponse` → `ProductPage`; see the `ToSchema` impl). Routes name it
/// with a type alias (`type ProductPage = PageResponse<ProductResponse>;`), since
/// `#[utoipa::path]` treats a generic `body =` differently.
#[derive(Serialize)]
pub(crate) struct PageResponse<T> {
    items: Vec<T>,
    /// Whether there are more after these.
    has_more: bool,
    /// Pass as `cursor` for the next page; `null` on the last page. Opaque: don't parse it.
    next_cursor: Option<String>,
}

impl<T> PageResponse<T> {
    /// The response for `page`: each item turned into its DTO by `item`, and the cursor
    /// of the next page made from the last item's `id` and the list's `scope`.
    pub(crate) fn new<U, Id: Display>(
        page: Page<U>,
        scope: &ListScope,
        id: impl Fn(&U) -> Id,
        item: impl FnMut(U) -> Result<T, ApiError>,
    ) -> Result<Self, ApiError> {
        let next_cursor = next_cursor(&page, scope, id)?;
        Ok(Self {
            items: page.items.into_iter().map(item).collect::<Result<_, _>>()?,
            has_more: page.has_more,
            next_cursor,
        })
    }

    /// The response for `page` with `items` already made, for DTOs whose making is
    /// async (asset URLs): one per page item, in order.
    pub(crate) fn with_items<U, Id: Display>(
        page: &Page<U>,
        scope: &ListScope,
        id: impl Fn(&U) -> Id,
        items: Vec<T>,
    ) -> Result<Self, ApiError> {
        Ok(Self {
            items,
            has_more: page.has_more,
            next_cursor: next_cursor(page, scope, id)?,
        })
    }
}

/// What a request's filters were, so the next page's cursor only continues this list.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct ListScope {
    /// The filter parameters, sorted, so their order in the URL doesn't matter.
    filters: Vec<(String, String)>,
}

/// A cursor's content: `v` is the format (bumped if it changes), `after` the last
/// item's ID. Encoded as base64url JSON; clients never read it.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    v: u8,
    after: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    filters: Vec<(String, String)>,
}

impl Cursor {
    const VERSION: u8 = 1;

    fn encode(&self) -> Result<String, ApiError> {
        let json = serde_json::to_vec(self).map_err(ApiError::internal)?;
        Ok(URL_SAFE_NO_PAD.encode(json))
    }

    /// The ID after which `scope`'s list continues, or `None` for a cursor that isn't
    /// one of ours, is from another version, or was made with other filters.
    fn decode<Id: FromStr>(cursor: &str, scope: &ListScope) -> Option<Id> {
        let json = URL_SAFE_NO_PAD.decode(cursor).ok()?;
        let cursor: Cursor = serde_json::from_slice(&json).ok()?;
        if cursor.v != Self::VERSION || cursor.filters != scope.filters {
            return None;
        }
        cursor.after.parse().ok()
    }
}

/// A cursor that continues `scope`'s list after `after`, for lists that page by
/// something other than their items' IDs (search pages by offset, design note 0019).
pub(crate) fn cursor_at(scope: &ListScope, after: impl Display) -> Result<String, ApiError> {
    Cursor {
        v: Cursor::VERSION,
        after: after.to_string(),
        filters: scope.filters.clone(),
    }
    .encode()
}

/// The cursor of the page after `page`, when there are more.
fn next_cursor<U, Id: Display>(
    page: &Page<U>,
    scope: &ListScope,
    id: impl Fn(&U) -> Id,
) -> Result<Option<String>, ApiError> {
    let Some(last) = page.items.last().filter(|_| page.has_more) else {
        return Ok(None);
    };
    Cursor {
        v: Cursor::VERSION,
        after: id(last).to_string(),
        filters: scope.filters.clone(),
    }
    .encode()
    .map(Some)
}

/// Written out rather than derived: utoipa's derive names a generic schema
/// `PageResponse_ProductSummaryResponse` and copies the item's schema into it, so the
/// item would lose its own named schema (and its type in the SDK).
impl<T: ToSchema> PartialSchema for PageResponse<T> {
    fn schema() -> RefOr<Schema> {
        ObjectBuilder::new()
            .description(Some("A page of a list, newest first (design note 0012)."))
            .property(
                "items",
                ArrayBuilder::new().items(Ref::from_schema_name(T::name())),
            )
            .required("items")
            .property(
                "has_more",
                ObjectBuilder::new()
                    .schema_type(Type::Boolean)
                    .description(Some("Whether there are more after these.")),
            )
            .required("has_more")
            .property(
                "next_cursor",
                ObjectBuilder::new()
                    .schema_type(SchemaType::from_iter([Type::String, Type::Null]))
                    .description(Some(
                        "Pass as `cursor` for the next page; `null` on the last page. \
                         Opaque: don't parse it.",
                    )),
            )
            .into()
    }
}

impl<T: ToSchema> ToSchema for PageResponse<T> {
    fn name() -> Cow<'static, str> {
        let item = T::name();
        let base = item.strip_suffix("Response").unwrap_or(&item);
        Cow::Owned(format!("{base}Page"))
    }

    fn schemas(schemas: &mut Vec<(String, RefOr<Schema>)>) {
        schemas.push((T::name().into_owned(), T::schema()));
        T::schemas(schemas);
    }
}

/// A list's query string, read but not yet parsed: call `parse` after authenticating,
/// like `JsonBody`. `cursor` and `limit` are the page; every other parameter goes to
/// `F`, the list's filters, which refuses unknown ones (`#[serde(deny_unknown_fields)]`),
/// the same as unknown body fields.
pub(crate) struct ListQuery<F = NoFilters> {
    query: Option<String>,
    _filters: PhantomData<fn() -> F>,
}

/// The filters of a list that has none: any parameter besides the page's is refused.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NoFilters {}

impl<S: Send + Sync, F> FromRequestParts<S> for ListQuery<F> {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(Self {
            query: parts.uri.query().map(str::to_string),
            _filters: PhantomData,
        })
    }
}

impl<F: DeserializeOwned> ListQuery<F> {
    /// The page asked for, the filters, and the scope to make the next cursor with. No
    /// `limit` is `PageLimit::DEFAULT`, one out of range is `422 invalid_limit`; a cursor
    /// this list (with these filters) didn't hand out is `400 invalid_cursor`; anything
    /// malformed or unknown is `400 invalid_query`.
    pub(crate) fn parse<Id: FromStr>(self) -> Result<(PageRequest<Id>, F, ListScope), ApiError> {
        let pairs: Vec<(String, String)> =
            serde_urlencoded::from_str(self.query.as_deref().unwrap_or(""))
                .map_err(ApiError::invalid_query)?;
        let (mut cursor, mut limit, mut filters) = (None, None, Vec::new());
        for (name, value) in pairs {
            let slot = match name.as_str() {
                "cursor" => &mut cursor,
                "limit" => &mut limit,
                _ => {
                    filters.push((name, value));
                    continue;
                }
            };
            if slot.replace(value).is_some() {
                return Err(ApiError::invalid_query(format!("duplicate field `{name}`")));
            }
        }

        filters.sort();
        let scope = ListScope {
            filters: filters.clone(),
        };
        let after = cursor
            .map(|c| Cursor::decode(&c, &scope).ok_or_else(ApiError::invalid_cursor))
            .transpose()?;
        let limit = match limit {
            None => PageLimit::DEFAULT,
            Some(limit) => {
                let limit = limit.parse().map_err(|_| {
                    ApiError::invalid_query(format!("`limit` is a whole number, not {limit:?}"))
                })?;
                PageLimit::new(limit)?
            }
        };
        // Back into a query string, so the filters parse with the same rules as any query.
        // `serde_html_form` collects a repeated key into a `Vec` field (search's `filter`)
        // and refuses one repeated for a single value.
        let rest = serde_urlencoded::to_string(&filters).map_err(ApiError::internal)?;
        let filters = serde_html_form::from_str(&rest).map_err(ApiError::invalid_query)?;
        Ok((PageRequest { after, limit }, filters, scope))
    }
}

/// `cursor` and `limit` in the OpenAPI documents: list it in a list route's
/// `params(...)`, next to its filters. The documents give a route with these exact
/// parameters the paging errors (`invalid_cursor`, `invalid_limit`).
pub(crate) struct PageParams;

impl utoipa::IntoParams for PageParams {
    fn into_params(_: impl Fn() -> Option<ParameterIn>) -> Vec<Parameter> {
        vec![
            ParameterBuilder::new()
                .name("cursor")
                .parameter_in(ParameterIn::Query)
                .required(Required::False)
                .description(Some(
                    "The previous page's `next_cursor`; absent for the first page.",
                ))
                .schema(Some(ObjectBuilder::new().schema_type(Type::String)))
                .build(),
            ParameterBuilder::new()
                .name("limit")
                .parameter_in(ParameterIn::Query)
                .required(Required::False)
                .description(Some(format!(
                    "Items per page, 1 to {} (default {}).",
                    PageLimit::MAX,
                    PageLimit::DEFAULT.get()
                )))
                .schema(Some(
                    ObjectBuilder::new()
                        .schema_type(Type::Integer)
                        .format(Some(SchemaFormat::KnownFormat(KnownFormat::Int32)))
                        .minimum(Some(1))
                        .maximum(Some(PageLimit::MAX)),
                ))
                .build(),
        ]
    }
}

/// `PageParams`' parameters, to recognise a list route by.
pub(crate) fn page_params() -> Vec<Parameter> {
    <PageParams as utoipa::IntoParams>::into_params(|| None)
}

#[cfg(test)]
mod tests {
    use hypha_core::ProductId;
    use uuid::Uuid;

    use super::*;

    #[derive(Debug, PartialEq, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Filters {
        status: Option<String>,
    }

    fn parse<F: DeserializeOwned>(query: &str) -> Result<(PageRequest<ProductId>, F), ApiError> {
        parse_scoped(query).map(|(page, filters, _)| (page, filters))
    }

    fn parse_scoped<F: DeserializeOwned>(
        query: &str,
    ) -> Result<(PageRequest<ProductId>, F, ListScope), ApiError> {
        ListQuery::<F> {
            query: Some(query.to_string()),
            _filters: PhantomData,
        }
        .parse()
    }

    /// The cursor a page of `ids` (with more after it) hands out under `scope`.
    fn cursor_after(id: ProductId, scope: &ListScope) -> String {
        let page = Page {
            items: vec![id],
            has_more: true,
        };
        next_cursor(&page, scope, |id| *id).unwrap().unwrap()
    }

    fn code<T>(result: Result<T, ApiError>) -> Option<&'static str> {
        result.err().map(|e| e.code())
    }

    #[test]
    fn the_page_and_the_filters_are_read_apart() {
        let id = ProductId::from_uuid(Uuid::from_u128(7));
        let (_, _, scope) = parse_scoped::<Filters>("status=draft").unwrap();
        let cursor = cursor_after(id, &scope);
        let (page, filters) =
            parse::<Filters>(&format!("status=draft&cursor={cursor}&limit=2")).unwrap();
        assert_eq!(
            page,
            PageRequest {
                after: Some(id),
                limit: PageLimit::new(2).unwrap()
            }
        );
        assert_eq!(
            filters,
            Filters {
                status: Some("draft".to_string())
            }
        );
    }

    #[test]
    fn an_empty_query_is_the_first_page() {
        let (page, _) = parse::<NoFilters>("").unwrap();
        assert_eq!(page, PageRequest::default());
    }

    #[test]
    fn unknown_parameters_are_refused() {
        assert_eq!(
            code(parse::<NoFilters>("status=draft")),
            Some("invalid_query")
        );
        assert_eq!(code(parse::<Filters>("sort=title")), Some("invalid_query"));
    }

    #[test]
    fn a_page_parameter_given_twice_is_refused() {
        assert_eq!(
            code(parse::<NoFilters>("limit=1&limit=2")),
            Some("invalid_query")
        );
    }

    #[test]
    fn limits_and_cursors_are_checked() {
        assert_eq!(code(parse::<NoFilters>("limit=ten")), Some("invalid_query"));
        assert_eq!(code(parse::<NoFilters>("limit=-1")), Some("invalid_query"));
        assert_eq!(code(parse::<NoFilters>("limit=0")), Some("invalid_limit"));
        assert_eq!(code(parse::<NoFilters>("limit=101")), Some("invalid_limit"));
        // A raw ID, as cursors were before 0016, isn't one any more.
        let product = "prod_00000000000000000000000000000001";
        assert_eq!(
            code(parse::<NoFilters>(&format!("cursor={product}"))),
            Some("invalid_cursor")
        );
        assert_eq!(
            code(parse::<NoFilters>("cursor=%%%")),
            Some("invalid_cursor")
        );
        assert_eq!(
            code(parse::<NoFilters>("cursor=bm9wZQ")),
            Some("invalid_cursor")
        );
    }

    #[test]
    fn a_cursor_is_opaque_and_continues_only_its_own_list() {
        let id = ProductId::from_uuid(Uuid::from_u128(7));
        let scope = ListScope::default();
        let cursor = cursor_after(id, &scope);
        assert!(!cursor.contains("prod_"), "{cursor}");
        let (page, _) = parse::<NoFilters>(&format!("cursor={cursor}")).unwrap();
        assert_eq!(page.after, Some(id));
        // Made with other filters: refused rather than continuing a different list.
        assert_eq!(
            code(parse::<Filters>(&format!("status=draft&cursor={cursor}"))),
            Some("invalid_cursor")
        );
        // Another list's IDs don't parse as this list's.
        let order = hypha_core::OrderId::from_uuid(Uuid::from_u128(7));
        let other = Cursor {
            v: Cursor::VERSION,
            after: order.to_string(),
            filters: vec![],
        }
        .encode()
        .unwrap();
        assert_eq!(
            code(parse::<NoFilters>(&format!("cursor={other}"))),
            Some("invalid_cursor")
        );
    }

    #[test]
    fn filters_in_another_order_are_the_same_list() {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        #[allow(dead_code)]
        struct Two {
            a: Option<String>,
            b: Option<String>,
        }
        let id = ProductId::from_uuid(Uuid::from_u128(7));
        let (_, _, scope) = parse_scoped::<Two>("b=2&a=1").unwrap();
        let cursor = cursor_after(id, &scope);
        let (page, _) = parse::<Two>(&format!("a=1&b=2&cursor={cursor}")).unwrap();
        assert_eq!(page.after, Some(id));
    }

    #[test]
    fn a_page_is_named_after_its_items() {
        #[derive(utoipa::ToSchema)]
        struct ThingResponse {}
        assert_eq!(PageResponse::<ThingResponse>::name(), "ThingPage");
    }
}
