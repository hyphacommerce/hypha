//! List filters on custom fields (design note 0033, §7): each `FieldFilter` becomes an
//! `EXISTS` (or `NOT EXISTS`) on `custom_field_values` for the listed row's ID, which
//! the values' primary key (definition, owner) answers per row.

use hypha_core::{FieldFilter, FieldFilterOp, FilterScalar};
use tokio_postgres::types::{ToSql, Type};

/// Parameters added after a query's fixed ones, numbered on from them.
pub(crate) struct Params {
    first: usize,
    values: Vec<(Box<dyn ToSql + Sync + Send>, Type)>,
}

impl Params {
    /// Parameters numbered from `$(fixed + 1)`, after a query's `fixed` ones.
    pub(crate) fn after(fixed: usize) -> Self {
        Self {
            first: fixed + 1,
            values: Vec::new(),
        }
    }

    /// Adds `value`; its placeholder (`$12`).
    fn add<T: ToSql + Sync + Send + 'static>(&mut self, value: T, ty: Type) -> String {
        self.values.push((Box::new(value), ty));
        format!("${}", self.first + self.values.len() - 1)
    }

    /// The parameters.
    pub(crate) fn as_slice(&self) -> Vec<(&(dyn ToSql + Sync), Type)> {
        self.values
            .iter()
            .map(|(v, t)| (v.as_ref() as &(dyn ToSql + Sync), t.clone()))
            .collect()
    }

    /// A query's `fixed` parameters followed by these.
    pub(crate) fn with_fixed<'a>(
        &'a self,
        fixed: &[(&'a (dyn ToSql + Sync), Type)],
    ) -> Vec<(&'a (dyn ToSql + Sync), Type)> {
        let mut all = fixed.to_vec();
        all.extend(self.as_slice());
        all
    }
}

/// `AND …` for every filter, on the rows whose ID is `owner` (`p.id`); empty for none.
pub(crate) fn field_predicates(
    filters: &[FieldFilter],
    owner: &str,
    params: &mut Params,
) -> String {
    filters
        .iter()
        .map(|filter| format!(" AND {}", predicate(filter, owner, params)))
        .collect()
}

fn predicate(filter: &FieldFilter, owner: &str, params: &mut Params) -> String {
    let definition = params.add(filter.definition.as_uuid(), Type::UUID);
    let value = |condition: &str| {
        format!(
            "EXISTS (SELECT 1 FROM custom_field_values cfv
                     WHERE cfv.definition_id = {definition} AND cfv.owner_id = {owner}{condition})"
        )
    };
    match &filter.op {
        FieldFilterOp::Exists(true) => value(""),
        FieldFilterOp::Exists(false) => format!("NOT {}", value("")),
        FieldFilterOp::AnyOf(scalars) => {
            let (texts, numbers) = split(scalars);
            let mut any = Vec::new();
            if !texts.is_empty() {
                any.push(format!(
                    "cfv.scalar_text = ANY({}::text[])",
                    params.add(texts, Type::TEXT_ARRAY)
                ));
            }
            if !numbers.is_empty() {
                any.push(format!(
                    "cfv.scalar_num = ANY({}::text[]::numeric[])",
                    params.add(numbers, Type::TEXT_ARRAY)
                ));
            }
            if any.is_empty() {
                // Any of nothing: no row matches.
                return "false".to_string();
            }
            value(&format!(" AND ({})", any.join(" OR ")))
        }
        FieldFilterOp::ContainsAny(items) => {
            let items: Vec<String> = items.iter().map(|i| i.to_string()).collect();
            value(&format!(
                " AND cfv.value @> ANY (SELECT jsonb_build_array(i::jsonb)
                                         FROM unnest({}::text[]) AS i)",
                params.add(items, Type::TEXT_ARRAY)
            ))
        }
        FieldFilterOp::Gt(s) => value(&compare(s, ">", params)),
        FieldFilterOp::Gte(s) => value(&compare(s, ">=", params)),
        FieldFilterOp::Lt(s) => value(&compare(s, "<", params)),
        FieldFilterOp::Lte(s) => value(&compare(s, "<=", params)),
    }
}

fn split(scalars: &[FilterScalar]) -> (Vec<String>, Vec<String>) {
    let mut texts = Vec::new();
    let mut numbers = Vec::new();
    for scalar in scalars {
        match scalar {
            FilterScalar::Text(t) => texts.push(t.clone()),
            FilterScalar::Number(n) => numbers.push(n.clone()),
        }
    }
    (texts, numbers)
}

/// ` AND cfv.scalar_num > $n::numeric`; text compares in byte order (the column's `C`
/// collation), as the domain does.
fn compare(scalar: &FilterScalar, op: &str, params: &mut Params) -> String {
    match scalar {
        FilterScalar::Text(t) => format!(
            " AND cfv.scalar_text {op} {}::text",
            params.add(t.clone(), Type::TEXT)
        ),
        FilterScalar::Number(n) => format!(
            " AND cfv.scalar_num {op} {}::text::numeric",
            params.add(n.clone(), Type::TEXT)
        ),
    }
}

#[cfg(test)]
mod tests {
    use hypha_core::CustomFieldDefinitionId;
    use uuid::Uuid;

    use super::*;

    #[test]
    fn placeholders_follow_the_fixed_parameters() {
        let mut params = Params::after(11);
        let sql = field_predicates(
            &[FieldFilter {
                definition: CustomFieldDefinitionId::from_uuid(Uuid::from_u128(1)),
                key: "custom.weight".to_string(),
                op: FieldFilterOp::Gte(FilterScalar::Number("1.5".to_string())),
            }],
            "p.id",
            &mut params,
        );
        assert!(sql.contains("cfv.definition_id = $12"));
        assert!(sql.contains("cfv.scalar_num >= $13::text::numeric"));
        assert_eq!(params.as_slice().len(), 2);
    }
}
