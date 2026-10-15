//! What every list route shares (design note 0012): the `cursor` and `limit` query
//! parameters, the cursor itself, and the page response. A new list takes a
//! `ListQuery<Filters>` (or `ListQuery` without filters), lists `PageParams` and its
//! filters in `params(...)`, and answers with a `PageResponse<ItemDto>`.
//!
//! Cursors are opaque to clients. Today one is the last item's ID; a list sorted by
//! something else would encode its sort key too, without changing the API.

use std::{borrow::Cow, fmt::Display, marker::PhantomData, str::FromStr};

use axum::{extract::FromRequestParts, http::request::Parts};
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
    /// of the next page made from the last item's `id`.
    pub(crate) fn new<U, Id: Display>(
        page: Page<U>,
        id: impl Fn(&U) -> Id,
        item: impl FnMut(U) -> Result<T, ApiError>,
    ) -> Result<Self, ApiError> {
        let next_cursor = page
            .has_more
            .then(|| page.items.last().map(|last| id(last).to_string()))
            .flatten();
        Ok(Self {
            items: page.items.into_iter().map(item).collect::<Result<_, _>>()?,
            has_more: page.has_more,
            next_cursor,
        })
    }
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
    /// The page asked for and the filters. No `limit` is `PageLimit::DEFAULT`, one out of
    /// range is `422 invalid_limit`; a cursor this list didn't hand out is
    /// `400 invalid_cursor`; anything malformed or unknown is `400 invalid_query`.
    pub(crate) fn parse<Id: FromStr>(self) -> Result<(PageRequest<Id>, F), ApiError> {
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

        let after = cursor
            .map(|c| c.parse().map_err(|_| ApiError::invalid_cursor()))
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
        let rest = serde_urlencoded::to_string(&filters).map_err(ApiError::internal)?;
        let filters = serde_urlencoded::from_str(&rest).map_err(ApiError::invalid_query)?;
        Ok((PageRequest { after, limit }, filters))
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
        ListQuery::<F> {
            query: Some(query.to_string()),
            _filters: PhantomData,
        }
        .parse()
    }

    fn code<T>(result: Result<T, ApiError>) -> Option<&'static str> {
        result.err().map(|e| e.code())
    }

    #[test]
    fn the_page_and_the_filters_are_read_apart() {
        let id = ProductId::from_uuid(Uuid::from_u128(7));
        let (page, filters) =
            parse::<Filters>(&format!("status=draft&cursor={id}&limit=2")).unwrap();
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
        let order = "ord_00000000000000000000000000000001";
        assert_eq!(
            code(parse::<NoFilters>(&format!("cursor={order}"))),
            Some("invalid_cursor")
        );
    }

    #[test]
    fn a_page_is_named_after_its_items() {
        #[derive(utoipa::ToSchema)]
        struct ThingResponse {}
        assert_eq!(PageResponse::<ThingResponse>::name(), "ThingPage");
    }
}
